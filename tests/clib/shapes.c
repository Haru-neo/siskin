#include <stdio.h>
#include <stdlib.h>
#include "shapes.h"

Vec2 vec_add(Vec2 a, Vec2 b) { Vec2 r = { a.x + b.x, a.y + b.y }; return r; }
float vec_dot(const Vec2* a, const Vec2* b) { return a->x * b->x + a->y * b->y; }
void vec_scale(Vec2* v, float k) { v->x *= k; v->y *= k; }
struct Pair pair_make(int a, double b) { struct Pair p = { a, b }; return p; }
double pair_sum(struct Pair p) { return p.a + p.b; }
void count_up(uint32_t* n, int16_t* small) { *n += 1; *small -= 1; }
int shape_visit(const Shape* s, int value) {
    if (!s->on_visit) return -1;
    return s->on_visit(value, s->name);
}
static int twice(int v, const char* label) { (void)label; return v * 2; }
static int thrice(int v, const char* label) { (void)label; return v * 3; }
Visit pick_visit(int which) { return which ? thrice : twice; }
Shape* shape_list(int n) {
    Shape* s = calloc((size_t)n, sizeof(Shape));
    for (int i = 0; i < n; i++) { s[i].kind = KIND_BOX; s[i].pos.x = (float)i; snprintf(s[i].name, sizeof s[i].name, "s%d", i); }
    return s;
}
#ifdef SHAPES_EXTRA
int shapes_extra(void) { return SHAPES_EXTRA; }
#endif

void image_grow(Imagep img, int by) {
    img->w += by;
    img->h += by;
    img->flags |= 0x4u;
}
