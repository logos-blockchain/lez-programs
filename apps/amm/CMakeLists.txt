cmake_minimum_required(VERSION 3.21)
project(AmmUiPlugin LANGUAGES CXX)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2)
qt_standard_project_setup(REQUIRES 6.8)

find_package(PkgConfig REQUIRED)
pkg_check_modules(BASE58 REQUIRED IMPORTED_TARGET libbase58)

include(CTest)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT.")
endif()

set(LOGOS_WALLET_SOURCE_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../shared/wallet"
    CACHE PATH "Path to the shared Logos wallet module"
)
set(LOGOS_WALLET_GENERATED_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/generated_code"
    CACHE PATH "Path to generated Logos SDK sources"
)
add_subdirectory("${LOGOS_WALLET_SOURCE_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/shared-wallet")

# ui_qml module with a hand-written C++ backend (QtRO .rep view contract +
# generated *SimpleSource/*ViewPluginBase). Mirrors the LEZ wallet UI module.
logos_module(
    NAME amm_ui
    REP_FILE src/AmmUiBackend.rep
    SOURCES
        src/AmmUiPluginInterface.h
        src/AmmUiPlugin.h
        src/AmmUiPlugin.cpp
        src/AmmUiBackend.h
        src/AmmUiBackend.cpp
        src/ActiveNetwork.h
        src/AmmClient.h
        src/AmmClient.cpp
        src/NewPositionRuntime.h
        src/NewPositionRuntime.cpp
        src/SwapRuntime.h
        src/SwapRuntime.cpp
    FIND_PACKAGES
        Qt6Gui
    LINK_LIBRARIES
        Qt6::Gui
        PkgConfig::BASE58
    LINK_TARGETS
        logos_wallet_access
    EXTERNAL_LIBS
        amm_client
)

if(BUILD_TESTING)
    add_executable(amm_new_position_runtime_test
        tests/cpp/NewPositionRuntimeTest.cpp
        src/NewPositionRuntime.cpp
    )
    target_include_directories(amm_new_position_runtime_test PRIVATE src)
    target_link_libraries(amm_new_position_runtime_test PRIVATE
        Qt6::Core
        PkgConfig::BASE58
        logos_wallet_access
    )
    add_test(NAME amm_new_position_runtime COMMAND amm_new_position_runtime_test)
endif()
