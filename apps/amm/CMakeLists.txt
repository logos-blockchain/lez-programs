cmake_minimum_required(VERSION 3.21)
project(AmmUiPlugin LANGUAGES CXX)

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
# LogosModule.cmake stages external libraries after this shared target is
# configured. Resolve the decoder from the staged dependency before adding the
# shared wallet module so its static target can link successfully.
if(NOT LOGOS_WALLET_IDL_DECODER_LIBRARY)
    set(amm_wallet_decoder_search_path "${CMAKE_CURRENT_SOURCE_DIR}/lib")
    if(DEFINED ENV{LOGOS_EXT_ROOT_WALLET_IDL_DECODER})
        set(amm_wallet_decoder_search_path
            "$ENV{LOGOS_EXT_ROOT_WALLET_IDL_DECODER}/lib")
    endif()
    find_library(AMM_WALLET_IDL_DECODER_LIBRARY
        NAMES wallet_idl_decoder
        PATHS "${amm_wallet_decoder_search_path}"
        NO_DEFAULT_PATH
    )
    if(AMM_WALLET_IDL_DECODER_LIBRARY)
        set(LOGOS_WALLET_IDL_DECODER_LIBRARY
            "${AMM_WALLET_IDL_DECODER_LIBRARY}"
            CACHE FILEPATH
            "wallet_idl_decoder library required by logos_wallet_access"
        )
    endif()
endif()
add_subdirectory("${LOGOS_WALLET_SOURCE_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/shared-wallet")

# ui_qml module with a hand-written C++ backend (QtRO .rep view contract +
# generated *SimpleSource/*ViewPluginBase). Mirrors the LEZ wallet UI module.
# The AMM business logic lives in the amm_module core module (declared as a
# dependency in metadata.json, reached via modules().amm_module in the backend),
# so the UI links no amm_ffi library of its own.
logos_module(
    NAME amm_ui
    REP_FILE src/AmmUiBackend.rep
    SOURCES
        src/AmmUiPluginInterface.h
        src/AmmUiPlugin.h
        src/AmmUiPlugin.cpp
        src/AmmUiBackend.h
        src/AmmUiBackend.cpp
        src/RegistryLoader.h
        src/RegistryLoader.cpp
    FIND_PACKAGES
        Qt6Gui
    LINK_LIBRARIES
        Qt6::Gui
        Qt6::Network
    LINK_TARGETS
        logos_wallet_access
    EXTERNAL_LIBS
        wallet_idl_decoder
)

set(LEZ_IDL_ARTIFACTS_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../../artifacts"
    CACHE PATH "Path to committed LEZ program IDL artifacts"
)
set(AMM_TOKEN_IDL "${LEZ_IDL_ARTIFACTS_DIR}/token-idl.json")
set(AMM_IDL "${LEZ_IDL_ARTIFACTS_DIR}/amm-idl.json")

foreach(idl_file IN ITEMS "${AMM_TOKEN_IDL}" "${AMM_IDL}")
    if(NOT EXISTS "${idl_file}")
        message(FATAL_ERROR "Committed IDL artifact not found: ${idl_file}")
    endif()
endforeach()

set_source_files_properties(
    "${AMM_TOKEN_IDL}"
    PROPERTIES QT_RESOURCE_ALIAS "idl/token-idl.json"
)
set_source_files_properties(
    "${AMM_IDL}"
    PROPERTIES QT_RESOURCE_ALIAS "idl/amm-idl.json"
)
qt_add_resources(amm_ui_module_plugin amm_ui_wallet_data
    PREFIX "/amm"
    FILES
        "${AMM_TOKEN_IDL}"
        "${AMM_IDL}"
)
