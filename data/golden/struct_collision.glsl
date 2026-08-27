// Regression: shader-crusher 0.7.1-alpha renamed a struct TYPE and a local
// VARIABLE to the same identifier. GLSL drivers lex a visible type name as
// TYPE_NAME, so `int t=0; while(t<9)` was read as a declaration and the
// compiler reported `'<' : syntax error`.
// Reported 2026-08-27, repro at 64/shader-crusher-struct-collision/.
#version 410
out vec4 out_color;
struct S { float v; };
uniform S uniform_struct;

float f() { int i = 0; while (i < 9) i += 1; return float(i); }

// a type declared inside a function must not take a name freed from outside
float g(float unused_here) {
	struct Inner { float a; float x; };
	Inner inner;
	inner.a = 2.0;
	inner.x = 3.0;
	return inner.a + inner.x;
}

void main() {
	S s;
	s.v = f() + g(1.0) + uniform_struct.v;
	out_color = vec4(s.v);
}
