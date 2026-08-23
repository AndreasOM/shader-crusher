// Test case for unary operators and unnecessary parentheses
// Verifies that unary operators don't cause excessive parentheses
//
// Expected: -x NOT -(x)
// Expected: !flag NOT !(flag)

#version 410

void main() {
    float x = 1.0;
    float y = 2.0;
    bool flag = true;

    // Test 1: Unary minus
    // Should output: -x (not -(x))
    float negX = -x;

    // Test 2: Unary minus in expression
    // Should output: -x+y or -(x)+y
    float result1 = -x + y;

    // Test 3: Multiple unary operators
    // Should output: --x (double negative, rare but valid)
    float doubleNeg = - -x;

    // Test 4: Unary not (boolean)
    // Should output: !flag (not !(flag))
    bool notFlag = !flag;

    // Test 5: Unary in multiplication
    // Should output: 2.0*-x or similar
    float result2 = 2.0 * -x;

    // Test 6: Unary in comparison
    // Should output: -x>-y or similar
    bool compare = -x > -y;

    // Test 7: Unary with field access
    // Should output: -x or -(vec.x) depending on context
    vec2 v = vec2(1.0, 2.0);
    float negVX = -v.x;

    // Test 8: Pre-increment (unary)
    int counter = 0;
    int preInc = ++counter;

    // Test 9: Post-increment
    int postInc = counter++;

    gl_FragColor = vec4(negX, result1, result2, 1.0);
}
