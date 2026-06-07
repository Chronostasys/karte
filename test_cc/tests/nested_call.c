int add(int a, int b) { return a + b; }
int mul(int a, int b) { return a * b; }
int main() {
    return add(mul(3, 4), mul(5, 6));
}
