cmake_minimum_required(VERSION 3.21)
project(TokenUiPlugin LANGUAGES CXX)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2)
qt_standard_project_setup(REQUIRES 6.8)

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
# The token prototype owns no wallet implementation; it links the repository's
# shared wallet access library and consumes its compiled Logos.Wallet QML module.
logos_module(
    NAME token_ui
    REP_FILE src/TokenUiBackend.rep
    SOURCES
        src/TokenUiPluginInterface.h
        src/TokenUiPlugin.h
        src/TokenUiPlugin.cpp
        src/TokenUiBackend.h
        src/TokenUiBackend.cpp
    FIND_PACKAGES
        Qt6Gui
    LINK_LIBRARIES
        Qt6::Gui
    LINK_TARGETS
        logos_wallet_access
)
