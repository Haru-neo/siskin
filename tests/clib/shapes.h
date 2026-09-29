/* C structs, constants, pointers and function pointers for tests/cstruct.skn. */
#ifndef SHAPES_H
#define SHAPES_H
#include <stdint.h>
#include "shapes_types.h"

typedef struct Vec2 { float x, y; } Vec2;
struct Pair { int a; double b; };
typedef union { float f[2]; uint32_t bits[2]; } Both;
typedef int (*Visit)(int value, const char* label);

typedef struct Shape {
    enum Kind kind;
    char name[8];
    Vec2 pos;
    float size[2];
    const char* note;
    Visit on_visit;
    Color color;
} Shape;

/* An unnamed struct with a pointer typedef, the way libpng declares `png_image, *png_imagep`. */
typedef struct { int w, h; uint32_t flags; } Image, *Imagep;

Vec2 vec_add(Vec2 a, Vec2 b);
float vec_dot(const Vec2* a, const Vec2* b);
void vec_scale(Vec2* v, float k);
struct Pair pair_make(int a, double b);
double pair_sum(struct Pair p);
void count_up(uint32_t* n, int16_t* small);
int shape_visit(const Shape* s, int value);
Visit pick_visit(int which);
Shape* shape_list(int n);
void image_grow(Imagep img, int by);
#ifdef SHAPES_EXTRA
int shapes_extra(void);
#endif
#endif
