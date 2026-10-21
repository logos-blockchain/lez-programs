cmake_minimum_required(VERSION 3.14)
project(OracleModuleTests LANGUAGES CXX)

include(LogosTest)

logos_test(
    NAME oracle_module_tests
    MODULE_SOURCES
        ../src/oracle_module_impl.cpp
        ../src/oracle_module_support.cpp
    TEST_SOURCES
        main.cpp
        oracle_module_impl_test.cpp
        oracle_module_support_test.cpp
    MOCK_C_SOURCES
        mocks/mock_oracle_ffi.cpp
    GENERATED_SOURCES
        ../generated_code/lez_core_api.cpp
    EXTRA_INCLUDES
        ../src
        ../ffi/include
        ../generated_code
)
