cmake_minimum_required(VERSION 3.14)
project(AmmModuleTests LANGUAGES CXX)

include(LogosTest)

find_library(AMM_FFI_LIBRARY NAMES amm_ffi
    PATHS "${CMAKE_CURRENT_SOURCE_DIR}/../lib" NO_DEFAULT_PATH REQUIRED)

logos_test(
    NAME amm_module_tests
    MODULE_SOURCES
        ../src/amm_fee_payer.cpp
        ../src/amm_module_impl.cpp
    TEST_SOURCES
        main.cpp
        amm_fee_payer_test.cpp
        amm_wallet_test.cpp
    EXTRA_INCLUDES
        ../src
        ../ffi/include
    EXTRA_LINK_LIBS
        ${AMM_FFI_LIBRARY}
)
