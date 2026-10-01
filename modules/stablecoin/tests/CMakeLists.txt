cmake_minimum_required(VERSION 3.14)
project(StablecoinModuleTests LANGUAGES CXX)

option(STABLECOIN_REAL_FFI_TESTS
       "Build the stateful module journey against the production FFI library"
       OFF)

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

if(STABLECOIN_REAL_FFI_TESTS)
    find_library(STABLECOIN_FFI_LIBRARY
        NAMES stablecoin_ffi
        PATHS "${CMAKE_CURRENT_SOURCE_DIR}/../lib"
        NO_DEFAULT_PATH
        REQUIRED)

    logos_test(
        NAME stablecoin_module_real_ffi_journey_tests
        MODULE_SOURCES
            ../src/stablecoin_module_impl.cpp
            ../src/stablecoin_module_support.cpp
        TEST_SOURCES
            main.cpp
            stablecoin_module_real_ffi_journey_test.cpp
        GENERATED_SOURCES
            ../generated_code/lez_core_api.cpp
        EXTRA_INCLUDES
            ../src
            ../ffi/include
            ../generated_code
        EXTRA_LINK_LIBS
            ${STABLECOIN_FFI_LIBRARY}
    )
endif()
