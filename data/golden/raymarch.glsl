// A typical 4k-intro style fragment shader: SDF raymarcher with lighting,
// fog and a camera. Exercises functions with parameters, nested loops,
// early returns, swizzles, const arrays, ternaries, compound assignments.
#version 330 core

uniform float iTime;
uniform vec2 iResolution;
out vec4 fragColor;

const float PI = 3.14159265;
const int MAX_STEPS = 96;
const float MAX_DIST = 60.0;
const float SURF_EPS = 0.001;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float fbm(vec2 p) {
    float sum = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 5; i++) {
        sum += amp * noise(p);
        p = p * 2.03 + vec2(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

mat2 rot(float a) {
    float s = sin(a), c = cos(a);
    return mat2(c, -s, s, c);
}

float sdSphere(vec3 p, float r) {
    return length(p) - r;
}

float sdBox(vec3 p, vec3 b) {
    vec3 q = abs(p) - b;
    return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
}

float sdTorus(vec3 p, vec2 t) {
    vec2 q = vec2(length(p.xz) - t.x, p.y);
    return length(q) - t.y;
}

float smin(float a, float b, float k) {
    float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

vec2 map(vec3 p) {
    float ground = p.y + 1.0 + 0.3 * fbm(p.xz * 0.5 + iTime * 0.1);
    vec3 q = p;
    q.xz *= rot(iTime * 0.3);
    float sphere = sdSphere(q - vec3(0.0, 0.5 + 0.3 * sin(iTime), 0.0), 1.0);
    float box = sdBox(q - vec3(2.5, 0.0, 0.0), vec3(0.7));
    float torus = sdTorus(q - vec3(-2.5, 0.0, 0.0), vec2(1.0, 0.3));
    float objects = smin(sphere, box, 0.5);
    objects = min(objects, torus);
    float material = objects < ground ? 1.0 : 0.0;
    return vec2(min(objects, ground), material);
}

vec3 calcNormal(vec3 p) {
    vec2 e = vec2(SURF_EPS, 0.0);
    return normalize(vec3(
        map(p + e.xyy).x - map(p - e.xyy).x,
        map(p + e.yxy).x - map(p - e.yxy).x,
        map(p + e.yyx).x - map(p - e.yyx).x));
}

float softShadow(vec3 ro, vec3 rd, float mint, float maxt, float k) {
    float res = 1.0;
    float t = mint;
    for (int i = 0; i < 32; i++) {
        float h = map(ro + rd * t).x;
        if (h < SURF_EPS) return 0.0;
        res = min(res, k * h / t);
        t += clamp(h, 0.02, 0.5);
        if (t > maxt) break;
    }
    return res;
}

float ambientOcclusion(vec3 p, vec3 n) {
    float occ = 0.0;
    float sca = 1.0;
    for (int i = 0; i < 5; i++) {
        float h = 0.01 + 0.12 * float(i) / 4.0;
        float d = map(p + h * n).x;
        occ += (h - d) * sca;
        sca *= 0.95;
    }
    return clamp(1.0 - 3.0 * occ, 0.0, 1.0);
}

vec2 raymarch(vec3 ro, vec3 rd) {
    float t = 0.0;
    float material = -1.0;
    for (int i = 0; i < MAX_STEPS; i++) {
        vec3 p = ro + rd * t;
        vec2 d = map(p);
        if (d.x < SURF_EPS) {
            material = d.y;
            break;
        }
        t += d.x;
        if (t > MAX_DIST) {
            material = -1.0;
            break;
        }
    }
    return vec2(t, material);
}

vec3 shade(vec3 p, vec3 n, vec3 rd, float material) {
    vec3 lightDir = normalize(vec3(0.6, 0.7, -0.5));
    vec3 albedo = material > 0.5 ? vec3(0.9, 0.4, 0.2) : vec3(0.2, 0.3, 0.25) + 0.1 * fbm(p.xz * 2.0);
    float diffuse = max(dot(n, lightDir), 0.0);
    float shadow = softShadow(p + n * 0.02, lightDir, 0.02, 10.0, 8.0);
    float ao = ambientOcclusion(p, n);
    vec3 half_vec = normalize(lightDir - rd);
    float spec = pow(max(dot(n, half_vec), 0.0), 32.0);
    vec3 color = albedo * (0.2 * ao + diffuse * shadow) + spec * shadow * 0.5;
    return color;
}

void main() {
    vec2 uv = (gl_FragCoord.xy - 0.5 * iResolution) / iResolution.y;
    vec3 ro = vec3(0.0, 1.5, -6.0);
    vec3 target = vec3(0.0, 0.0, 0.0);
    vec3 forward = normalize(target - ro);
    vec3 right = normalize(cross(vec3(0.0, 1.0, 0.0), forward));
    vec3 up = cross(forward, right);
    vec3 rd = normalize(uv.x * right + uv.y * up + 1.8 * forward);

    vec2 hit = raymarch(ro, rd);
    vec3 color;
    if (hit.y < 0.0) {
        color = mix(vec3(0.5, 0.7, 0.9), vec3(0.1, 0.2, 0.4), clamp(rd.y * 2.0 + 0.5, 0.0, 1.0));
    } else {
        vec3 p = ro + rd * hit.x;
        vec3 n = calcNormal(p);
        color = shade(p, n, rd, hit.y);
        float fog = 1.0 - exp(-0.02 * hit.x * hit.x);
        color = mix(color, vec3(0.5, 0.7, 0.9), fog);
    }
    color = pow(color, vec3(1.0 / 2.2));
    fragColor = vec4(color, 1.0);
}
