#include <iostream>

int main() {
    int number;
    std::cout << "Enter an integer: ";
    if (std::cin >> number) {
        if (number % 2 == 0) {
            std::cout << number << " is even." << std::endl;
        } else {
            std::cout << number << " is odd." << std::endl;
        }
    } else {
        std::cerr << "Invalid input." << std::endl;
        return 1;
    }
    return 0;
}
