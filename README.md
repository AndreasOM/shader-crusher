# Danger - Status
Works for me, but might wipe your harddisk.

# What?

Takes a glsl shader, removes white space, comments, etc, and replaces symbols/identifiers/type names by shorter ones that compress better.

# Why?

I got tired of installing mono to get shader-minifier working,
and needed something that I could embedd into my tools.

# Why rust?

Because.
And I want to learn rust.

And it's portable, and fast, and future proof.

# Usage

## Commandline

```
cargo run --help

cargo run -- --input shader.glsl --output shader_crushed.glsl
```

Use ```--blacklist "dont,crush,these"```
or
```glsl

// code

#pragma SHADER_CRUSHER_OFF

// code

#pragma SHADER_CRUSHER_ON

// code
```
to keep certain identifiers untouched, e.g. uniforms that you need to resolve externaly.
Keywords, built-in functions, and 'main' are automatically blacklisted.

## Embedded/Linked

From C/C++

```c++
shader_crusher::ShaderCrusher* pShaderCrusher = shader_crusher::shadercrusher_new();
shader_crusher::shadercrusher_set_input( pShaderCrusher, fragmentString.c_str() );
shader_crusher::shadercrusher_crush( pShaderCrusher );
char* pOutput = shader_crusher::shadercrusher_get_ouput( pShaderCrusher );
fragmentString = std::string( pOutput );
shader_crusher::shadercrusher_free_ouput( pShaderCrusher, pOutput );
shader_crusher::shadercrusher_free( pShaderCrusher );
```
 Don't forget do include the cbindgen generated header file, and link against the lib.


# Stats

 I only used it on my shaders so far, but on average the crushed size is 60%, or 40% if you further compress (e.g. with UPX/Crinkler/kkrunchy).

# Soon

(because I want it)

 - Allow blacklist via c-api.
 - Add revalidation of output.
 - Run against piglet suite.
 - CI system with testing

# Future

(aka not going to happen anytime soon)

 - Dead code removal
 - Smart replacement of repeated blocks
 - Multishader passes for smart extraction of shared code


# Help

- Run this against your shader, and see if it breaks anything, and what compression ratio you get.
- Fix [#52](https://github.com/phaazon/glsl/issues/52) in the glsl crate (too many braces).
- Fix [#110](https://github.com/phaazon/glsl/issues/110) in the glsl crate (whitespace in defines).
- The swizzling blacklist is totally wrong, but gets the job done for now.
- '#define's could be fixed, but I was lazy.

