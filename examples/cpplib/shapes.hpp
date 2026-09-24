#pragma once
#include <string>

namespace geo {

class Shape {
public:
    virtual ~Shape() {}
    virtual double area() const = 0;
    virtual std::string kind() const = 0;
};

class Circle : public Shape {
    double r;
public:
    Circle(double radius);
    double area() const override;
    std::string kind() const override;
    inline double radius() const { return r; }
    void grow(double by);
};

class Rect : public Shape {
    double w, h;
public:
    Rect(double w_, double h_);
    double area() const override;
    std::string kind() const override;
};

double hypot2(double a, double b);
std::string greet(const std::string& who);
int char_count(const std::string& s);
}
