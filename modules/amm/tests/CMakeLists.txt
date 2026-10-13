cmake_minimum_required(VERSION 3.14)
project(AmmModuleTests LANGUAGES CXX)

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
