cmake_minimum_required(VERSION 3.14)
project(StablecoinModuleTests LANGUAGES CXX)

include(LogosTest)

logos_test(
    NAME stablecoin_module_support_tests
    MODULE_SOURCES
        ../src/stablecoin_module_support.cpp
    TEST_SOURCES
        main.cpp
        stablecoin_module_support_test.cpp
    EXTRA_INCLUDES
        ../src
)
