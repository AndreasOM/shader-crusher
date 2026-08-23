// Regression fixture: every construct that an earlier shader-crusher got wrong.
//  - macro parameters / bodies / macro-referenced identifiers (SQ, NEG, HALF, scale_factor)
//  - layout qualifier identifiers (location)
//  - precision statement
//  - built-in struct members (gl_LightSource[0].position)
//  - `a - -b` spacing, comma expression in initializer / call argument
//  - legacy builtin functions (texture2D, ftransform)
#version 120
#define SQ(v) ((v)*(v))
#define NEG(q) -(q)
#define HALF (scale_factor*0.5)
uniform float scale_factor;
uniform sampler2D base_texture;
precision mediump float;
struct Light { vec3 position; float intensity; };
uniform Light light_src;
varying vec2 tex_coord;
float helper_fn(float value_in) { float tmp_local = value_in * 2.0; return tmp_local; }
void main(void) {
  float tmp_local = 0.5;
  vec2 uv = tex_coord.xy * 100.0 + 0.001;
  vec3 p = light_src.position + gl_LightSource[0].position.xyz;
  float x = p.x + p.y, a = SQ(uv.x + 1.0) + NEG(tmp_local) + HALF + helper_fn(0.25) - -tmp_local;
  vec4 color = texture2D(base_texture, uv) * (x, a);
  color.rgb = color.bgr * light_src.intensity;
  gl_FragColor = (color, vec4(x, a, uv, 1.0e-5));
  gl_Position = ftransform();
}
