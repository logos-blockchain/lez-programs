cmake_minimum_required(VERSION 3.21)
project(AmmUiPlugin LANGUAGES CXX)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2)
qt_standard_project_setup(REQUIRES 6.8)

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
        src/ActiveNetwork.cpp
        src/WalletIdlDecoder.h
        src/WalletIdlDecoder.cpp
    FIND_PACKAGES
        Qt6Gui
        Qt6Network
    LINK_LIBRARIES
        Qt6::Gui
        Qt6::Network
    LINK_TARGETS
        logos_wallet_access
    EXTERNAL_LIBS
        wallet_idl_decoder
)

set_source_files_properties(
    config/idl/token-idl.json
    PROPERTIES QT_RESOURCE_ALIAS "idl/token-idl.json"
)
set_source_files_properties(
    config/idl/amm-idl.json
    PROPERTIES QT_RESOURCE_ALIAS "idl/amm-idl.json"
)
qt_add_resources(amm_ui_module_plugin amm_ui_wallet_data
    PREFIX "/amm"
    FILES
        config/networks.json
        config/idl/token-idl.json
        config/idl/amm-idl.json
)

include(CTest)
if(BUILD_TESTING)
    find_package(Qt6 6.8 REQUIRED COMPONENTS Test)
    add_executable(amm_active_network_test
        tests/cpp/ActiveNetworkTest.cpp
        src/ActiveNetwork.cpp
    )
    set_target_properties(amm_active_network_test PROPERTIES AUTOMOC ON)
    target_include_directories(amm_active_network_test PRIVATE src)
    target_link_libraries(amm_active_network_test PRIVATE Qt6::Core Qt6::Test)
    add_test(NAME amm_active_network COMMAND amm_active_network_test)
endif()
