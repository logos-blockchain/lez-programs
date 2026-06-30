cmake_minimum_required(VERSION 3.14)
project(AmmUiPlugin LANGUAGES CXX)

include(GNUInstallDirs)

add_compile_definitions(AMM_UI_ASSET_DIR="${CMAKE_INSTALL_FULL_LIBDIR}")

set(AMM_UI_PROGRAM_DIR "" CACHE PATH "Directory containing amm.bin and token.bin")
if(NOT AMM_UI_PROGRAM_DIR AND DEFINED ENV{AMM_UI_PROGRAM_DIR})
    set(AMM_UI_PROGRAM_DIR "$ENV{AMM_UI_PROGRAM_DIR}" CACHE PATH "Directory containing amm.bin and token.bin" FORCE)
endif()

if(AMM_UI_PROGRAM_DIR)
    foreach(_program_binary IN ITEMS amm.bin token.bin)
        if(NOT EXISTS "${AMM_UI_PROGRAM_DIR}/${_program_binary}")
            message(FATAL_ERROR "Required AMM UI program binary not found: ${AMM_UI_PROGRAM_DIR}/${_program_binary}")
        endif()
    endforeach()
else()
    message(WARNING "AMM_UI_PROGRAM_DIR not set; amm.bin and token.bin will not be installed")
endif()

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT.")
endif()

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
        src/AccountModel.h
        src/AccountModel.cpp
    FIND_PACKAGES
        Qt6Gui
        Qt6Network
    LINK_LIBRARIES
        Qt6::Gui
        Qt6::Network
        ${CMAKE_DL_LIBS}
)

install(DIRECTORY config
    DESTINATION ${CMAKE_INSTALL_LIBDIR}
)

if(AMM_UI_PROGRAM_DIR)
    install(FILES
        ${AMM_UI_PROGRAM_DIR}/amm.bin
        ${AMM_UI_PROGRAM_DIR}/token.bin
        DESTINATION ${CMAKE_INSTALL_LIBDIR}/programs
    )
endif()
