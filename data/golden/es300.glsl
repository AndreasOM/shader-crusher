// GLSL ES 3.00 fragment shader: precision statements, in/out, layout
// qualifiers, a uniform block with an instance name, switch, do-while,
// integer/bit operations, struct arrays, array constructors.
#version 300 es
precision highp float;
precision mediump int;

layout(location = 0) out vec4 frag_color;

in vec2 v_uv;
flat in int v_mode;

uniform sampler2D u_texture;
uniform Params {
    vec4 tint;
    float threshold;
    int iterations;
} params;

struct Light {
    vec3 direction;
    vec3 color;
    float intensity;
};

uniform Light u_lights[2];

const vec3 weights[3] = vec3[3](vec3(0.299, 0.587, 0.114), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0));

float luma(vec3 rgb, int mode) {
    return dot(rgb, weights[mode]);
}

vec3 applyLights(vec3 base, vec3 normal) {
    vec3 result = vec3(0.0);
    for (int i = 0; i < 2; ++i) {
        float ndl = max(dot(normal, normalize(u_lights[i].direction)), 0.0);
        result += base * u_lights[i].color * u_lights[i].intensity * ndl;
    }
    return result;
}

uint checksum(uvec2 p) {
    uint h = 2166136261u;
    h = (h ^ p.x) * 16777619u;
    h = (h ^ p.y) * 16777619u;
    return h >> 3u;
}

void main() {
    vec4 sample_color = texture(u_texture, v_uv);
    vec3 rgb = sample_color.rgb * params.tint.rgb;
    float l = luma(rgb, v_mode);
    vec3 normal = normalize(vec3(v_uv - 0.5, 1.0));
    vec3 lit = applyLights(rgb, normal);

    int k = 0;
    do {
        lit *= 0.98;
        k++;
    } while (k < params.iterations);

    switch (v_mode) {
        case 0:
            lit = vec3(l);
            break;
        case 1:
            lit = l > params.threshold ? lit : vec3(0.0);
            break;
        default:
            lit = mix(lit, vec3(l), 0.5);
    }

    uint h = checksum(uvec2(gl_FragCoord.xy));
    float dither = float(h & 255u) / 255.0 - 0.5;
    lit += dither * 0.01;

    frag_color = vec4(lit, sample_color.a * params.tint.a);
}
