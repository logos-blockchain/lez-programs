cmake_minimum_required(VERSION 3.14)
project(TokenModuleTests LANGUAGES CXX)

include(LogosTest)

logos_test(
    NAME token_instruction_transport_tests
    MODULE_SOURCES
        ../src/token_instruction_words.cpp
    TEST_SOURCES
        main.cpp
        token_instruction_words_test.cpp
    EXTRA_INCLUDES
        ../src
)
