#include <iostream>

int add(int a, int b) {
    return a + b;
}

int main() {
    int result = add(10, 20);
    std::cout << result << std::endl;
    return 0;
}
