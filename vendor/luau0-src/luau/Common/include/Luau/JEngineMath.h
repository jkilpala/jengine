// jEngine patch (vendor/luau0-src/JENGINE-PATCH.md): the maths a script's
// numbers go through, the same bits on every platform.
//
// `sin`, `pow` and the rest come from the platform's maths library, whose
// last bit differs between Windows, macOS, Linux, Android and the browser,
// so a game written in Lua played out differently on each. Every place
// Luau computes one - the `math` library, its fast calls, the `^` operator
// and the compiler's constant folding - calls through this table instead,
// which jEngine fills with Rust's `libm` (`jengine_core::scripting::script_math`)
// before it compiles a script: the very functions the browsers' Fengari
// calls into the engine's wasm module for. Until it is filled it holds the
// platform's own, so Luau works unchanged without jEngine.
#pragma once

#include <math.h>

extern "C"
{
    struct LuauJEngineMath
    {
        double (*f_sin)(double);
        double (*f_cos)(double);
        double (*f_tan)(double);
        double (*f_asin)(double);
        double (*f_acos)(double);
        double (*f_atan)(double);
        double (*f_atan2)(double, double);
        double (*f_exp)(double);
        double (*f_log)(double);
        double (*f_log2)(double);
        double (*f_log10)(double);
        double (*f_pow)(double, double);
        double (*f_sinh)(double);
        double (*f_cosh)(double);
        double (*f_tanh)(double);
    };

    // Defined in Ast/src/JEngineMath.cpp: the library linked last, so every
    // reference to it resolves on a linker that reads archives in order.
    extern LuauJEngineMath luau_jengine_math;
}

// `a ^ b`, `math.pow` and their folding, as one rule: the three exponents
// Luau's VM always special-cased for speed, then the shared `pow`. Fengari's
// `^` (editor/vendor/fengari) is patched to the same rule, so `x ^ 0.5` is
// `sqrt(x)` in both whether the exponent is a constant or not.
inline double luau_jengine_pow(double a, double b)
{
    return (b == 2.0) ? a * a : (b == 0.5) ? sqrt(a) : (b == 3.0) ? a * a * a : luau_jengine_math.f_pow(a, b);
}
