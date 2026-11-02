cmake_minimum_required(VERSION 3.14)
project(AmmModuleTests LANGUAGES CXX)

enable_testing()
option(AMM_REAL_FFI_TESTS "Build AMM module tests against the production FFI library" OFF)
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
        amm_module_read_test.cpp
        amm_module_submit_test.cpp
    MOCK_C_SOURCES
        mocks/mock_amm_ffi.cpp
    EXTRA_INCLUDES
        ../src
        ../ffi/include
)

if(AMM_REAL_FFI_TESTS)
    find_library(AMM_FFI_LIBRARY
        NAMES amm_ffi
        PATHS "${CMAKE_CURRENT_SOURCE_DIR}/../lib"
        NO_DEFAULT_PATH
        REQUIRED)
    logos_test(
        NAME amm_module_real_ffi_tests
        MODULE_SOURCES
            ../src/amm_module_impl.cpp
            ../src/amm_fee_payer.cpp
        TEST_SOURCES
            main.cpp
            amm_module_real_ffi_test.cpp
        EXTRA_INCLUDES
            ../src
            ../ffi/include
        EXTRA_LINK_LIBS
            ${AMM_FFI_LIBRARY}
    )
endif()
