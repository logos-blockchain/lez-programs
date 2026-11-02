cmake_minimum_required(VERSION 3.14)
project(AmmModuleTests LANGUAGES CXX)

enable_testing()
include(LogosTest)

logos_test(
    NAME amm_fee_payer_tests
    MODULE_SOURCES
        ../src/amm_fee_payer.cpp
    TEST_SOURCES
        main.cpp
        amm_fee_payer_test.cpp
    EXTRA_INCLUDES
        ../src
)

logos_test(
    NAME amm_module_tests
    MODULE_SOURCES
        ../src/amm_module_impl.cpp
        ../src/amm_fee_payer.cpp
    TEST_SOURCES
        main.cpp
        amm_module_regression_test.cpp
    MOCK_C_SOURCES
        mocks/mock_amm_ffi.cpp
    EXTRA_INCLUDES
        ../src
        ../ffi/include
)
