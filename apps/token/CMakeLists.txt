cmake_minimum_required(VERSION 3.14)
project(TokenUiPlugin LANGUAGES CXX)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT.")
endif()

# ui_qml module with a hand-written C++ backend (QtRO .rep view contract +
# generated *SimpleSource/*ViewPluginBase). Mirrors the LEZ wallet UI module.
logos_module(
    NAME token_ui
    REP_FILE src/TokenUiBackend.rep
    SOURCES
        src/TokenUiPluginInterface.h
        src/TokenUiPlugin.h
        src/TokenUiPlugin.cpp
        src/TokenUiBackend.h
        src/TokenUiBackend.cpp
        # Shared wallet UI, overlaid from apps/common/wallet-ui at build time.
        src/WalletBackendLogic.h
        src/AccountModel.h
        src/AccountModel.cpp
    FIND_PACKAGES
        Qt6Gui
        Qt6Network
    LINK_LIBRARIES
        Qt6::Gui
        Qt6::Network
)
