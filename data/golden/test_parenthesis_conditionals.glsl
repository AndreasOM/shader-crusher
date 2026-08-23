// Test case for excessive parentheses in conditional expressions
// Based on GitHub issue #52: https://github.com/phaazon/glsl/issues/52
//
// The bug generated output like: if ((((n)-(p))>(0.))&&((((u).y)<(n))&&(((u).y)>(p))))
// Expected clean output: if (n-p>0.&&u.y<n&&u.y>p)

#version 410

void main() {
    float n = 1.0;
    float p = 0.5;
    vec2 u = vec2(0.3, 0.7);

    // Test 1: Complex conditional with multiple && operators
    if ((n-p>0.)&&(u.y<n)&&(u.y>p)) {
        gl_FragColor = vec4(1.0, 0.0, 0.0, 1.0);
    }

    // Test 2: Complex conditional with || operators
    if ((n>1.0)||(p<0.5)||(u.x>0.5)) {
        gl_FragColor = vec4(0.0, 1.0, 0.0, 1.0);
    }

    // Test 3: Mixed && and || operators
    if (((n>0.5)&&(p<1.0))||((u.x>0.2)&&(u.y<0.8))) {
        gl_FragColor = vec4(0.0, 0.0, 1.0, 1.0);
    }

    // Test 4: Negation in conditionals
    if (!(n<p)&&!(u.x>1.0)) {
        gl_FragColor = vec4(1.0, 1.0, 0.0, 1.0);
    }
}
