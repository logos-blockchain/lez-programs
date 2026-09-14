cmake_minimum_required(VERSION 3.14)
project(StablecoinModuleTests LANGUAGES CXX)

include(LogosTest)

logos_test(
    NAME stablecoin_module_tests
    MODULE_SOURCES
        ../src/stablecoin_module_impl.cpp
        ../src/stablecoin_module_support.cpp
    TEST_SOURCES
        main.cpp
        stablecoin_module_impl_test.cpp
        stablecoin_module_support_test.cpp
    MOCK_C_SOURCES
        mocks/mock_stablecoin_ffi.cpp
    GENERATED_SOURCES
        ../generated_code/lez_core_api.cpp
    EXTRA_INCLUDES
        ../src
        ../ffi/include
        ../generated_code
)
