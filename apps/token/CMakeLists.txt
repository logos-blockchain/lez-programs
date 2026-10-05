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

# The wallet access library is shared by Basecamp UI modules. The generated
# Logos SDK directory is supplied by logos-module-builder at configure time.
set(LOGOS_WALLET_SOURCE_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../shared/wallet"
    CACHE PATH "Path to the shared Logos wallet module"
)
set(LOGOS_WALLET_GENERATED_DIR
    "${CMAKE_CURRENT_BINARY_DIR}/generated_code"
    CACHE PATH "Path to generated Logos SDK sources"
)
# LogosModule.cmake stages external libraries after this shared target is
# configured. Resolve the decoder from the staged dependency before adding the
# shared wallet module so its static target can link successfully.
if(NOT LOGOS_WALLET_IDL_DECODER_LIBRARY)
    set(token_wallet_decoder_search_path "${CMAKE_CURRENT_SOURCE_DIR}/lib")
    if(DEFINED ENV{LOGOS_EXT_ROOT_WALLET_IDL_DECODER})
        set(token_wallet_decoder_search_path
            "$ENV{LOGOS_EXT_ROOT_WALLET_IDL_DECODER}/lib")
    endif()
    find_library(TOKEN_WALLET_IDL_DECODER_LIBRARY
        NAMES wallet_idl_decoder
        PATHS "${token_wallet_decoder_search_path}"
        NO_DEFAULT_PATH
    )
    if(TOKEN_WALLET_IDL_DECODER_LIBRARY)
        set(LOGOS_WALLET_IDL_DECODER_LIBRARY
            "${TOKEN_WALLET_IDL_DECODER_LIBRARY}"
            CACHE FILEPATH
            "wallet_idl_decoder library required by logos_wallet_access"
        )
    endif()
endif()
add_subdirectory("${LOGOS_WALLET_SOURCE_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/shared-wallet")

# UI module with a hand-written C++ adapter. The adapter forwards token
# operations to token_module and owns only wallet-session/UI aggregation logic.
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
    EXTERNAL_LIBS
        wallet_idl_decoder
)
