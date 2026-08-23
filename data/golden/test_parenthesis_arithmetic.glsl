// Test case for operator precedence and unnecessary parentheses in arithmetic
// Verifies that the GLSL transpiler respects operator precedence
//
// Expected: Should NOT add unnecessary parentheses
// * and / have higher precedence than + and -

#version 410

void main() {
    float a = 1.0;
    float b = 2.0;
    float c = 3.0;
    float d = 4.0;

    // Test 1: Multiplication has precedence over addition
    // Should output: a+b*c (no parens needed)
    float result1 = a + b * c;

    // Test 2: Explicit parentheses to override precedence
    // Should output: (a+b)*c (parens needed and kept)
    float result2 = (a + b) * c;

    // Test 3: Multiplication then addition
    // Should output: a*b+c (no parens needed)
    float result3 = a * b + c;

    // Test 4: Division and subtraction
    // Should output: a/b-c (no parens needed)
    float result4 = a / b - c;

    // Test 5: Complex expression
    // Should output: a+b*c/d (no parens needed)
    float result5 = a + b * c / d;

    // Test 6: Explicit parens in complex expression
    // Should output: (a+b)*(c-d) (parens needed and kept)
    float result6 = (a + b) * (c - d);

    // Test 7: Nested operations
    // Should output: a*(b+c*d) (inner parens needed, outer not)
    float result7 = a * (b + c * d);

    gl_FragColor = vec4(result1, result2, result3, 1.0);
}
