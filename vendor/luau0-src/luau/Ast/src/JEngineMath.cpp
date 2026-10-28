// jEngine patch (vendor/luau0-src/JENGINE-PATCH.md): the table of maths
// functions Luau computes through, the platform's own until jEngine fills it.
#include "Luau/JEngineMath.h"

LuauJEngineMath luau_jengine_math = {
    ::sin, ::cos, ::tan, ::asin, ::acos, ::atan, ::atan2, ::exp, ::log, ::log2, ::log10, ::pow, ::sinh, ::cosh, ::tanh,
};
