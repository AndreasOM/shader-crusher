// Test case for field access (swizzling) and unnecessary parentheses
// Verifies that simple field access doesn't get wrapped in extra parens
//
// Expected: color.r NOT (color).r
// Expected: (color+offset).g (parens needed for expression)

#version 410

uniform vec4 color;
uniform vec2 position;

void main() {
    // Test 1: Simple field access
    // Should output: color.r (no parens around color)
    float r = color.r;
    float g = color.g;
    float b = color.b;

    // Test 2: Swizzling
    // Should output: color.rgb (no parens)
    vec3 rgb = color.rgb;

    // Test 3: Field access on expression (parens needed)
    // Should output: (color+vec4(0.1)).g
    float brightG = (color + vec4(0.1)).g;

    // Test 4: Chained field access
    // Should output: position.xy
    vec2 pos = position.xy;

    // Test 5: Field access in arithmetic
    // Should output: color.r+color.g (no extra parens)
    float sum = color.r + color.g;

    // Test 6: Field access on function result
    // Should output: normalize(color).r or similar
    float normR = normalize(color).r;

    // Test 7: Multiple swizzles in expression
    // Should output: position.x*position.y
    float area = position.x * position.y;

    gl_FragColor = vec4(r, g, b, 1.0);
}
