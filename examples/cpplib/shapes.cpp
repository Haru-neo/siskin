#include "shapes.hpp"
#include <cmath>
namespace geo {
Circle::Circle(double radius) : r(radius) {}
double Circle::area() const { return M_PI * r * r; }
std::string Circle::kind() const { return "원"; }
void Circle::grow(double by) { r += by; }
Rect::Rect(double w_, double h_) : w(w_), h(h_) {}
double Rect::area() const { return w * h; }
std::string Rect::kind() const { return "직사각형"; }
double hypot2(double a, double b) { return std::sqrt(a*a + b*b); }
std::string greet(const std::string& who) { return "안녕하세요, " + who + "님"; }
int char_count(const std::string& s) { return (int)s.size(); }
}
