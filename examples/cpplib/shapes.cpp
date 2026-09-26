#include "shapes.hpp"
#include <cmath>
namespace geo {
Circle::Circle(double radius) : r(radius) {}
double Circle::area() const { return M_PI * r * r; }
std::string Circle::kind() const { return "circle"; }
void Circle::grow(double by) { r += by; }
Rect::Rect(double w_, double h_) : w(w_), h(h_) {}
double Rect::area() const { return w * h; }
std::string Rect::kind() const { return "rectangle"; }
double hypot2(double a, double b) { return std::sqrt(a*a + b*b); }
std::string greet(const std::string& who) { return "Hello, " + who + "!"; }
int char_count(const std::string& s) { return (int)s.size(); }
}
