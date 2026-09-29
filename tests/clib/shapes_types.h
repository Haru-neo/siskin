/* Included by shapes.h with quotes, so Siskin follows it. */
#ifndef SHAPES_TYPES_H
#define SHAPES_TYPES_H
typedef struct { unsigned char rgba[4]; } Color;
enum Kind { KIND_CIRCLE = 3, KIND_BOX, KIND_OTHER = 1 << 4 };
#define SHAPES_MAGIC 0x1234u
#define SHAPES_NAME "shapes"
#define SHAPES_PI 3.25
#endif
