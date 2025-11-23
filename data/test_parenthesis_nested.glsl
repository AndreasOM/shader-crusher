// Test case for deeply nested expressions and parentheses
// Verifies that complex nested expressions don't generate excessive parens
//
// This is a stress test for the parenthesis generation logic

#version 410

void main() {
    float a = 1.0;
    float b = 2.0;
    float c = 3.0;
    float d = 4.0;
    float e = 5.0;

    // Test 1: Deeply nested arithmetic
    // Should respect precedence and only use necessary parens
    float nested1 = a + b * (c + d * (e - a));

    // Test 2: Multiple levels of parentheses
    // Should keep necessary parens, remove unnecessary ones
    float nested2 = ((a + b) * (c - d)) / ((e + a) - (b * c));

    // Test 3: Nested function calls
    // Should output cleanly without wrapping each call
    float nested3 = max(min(a, b), min(c, d));

    // Test 4: Nested field access and operations
    vec2 v1 = vec2(a, b);
    vec2 v2 = vec2(c, d);
    float nested4 = (v1.x + v2.y) * (v1.y - v2.x);

    // Test 5: Nested conditionals in ternary
    // Should use parens where needed for clarity
    float nested5 = (a > b) ? (c > d ? e : a) : (b > c ? d : e);

    // Test 6: Complex boolean expression
    bool nested6 = ((a > b) && (c < d)) || ((e >= a) && (b <= c));

    // Test 7: Nested array/vector constructors
    vec4 nested7 = vec4(vec3(a, b, c), d);

    // Test 8: Mix of everything
    float nested8 = max(a * b + c, min(d / e, (a + b) * (c - d)));

    gl_FragColor = vec4(nested1, nested2, nested3, 1.0);
}
