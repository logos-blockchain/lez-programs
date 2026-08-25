cmake_minimum_required(VERSION 3.14)
project(AmmModulePlugin LANGUAGES CXX)

# Include the Logos Module CMake helper (provided by logos-module-builder).
if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
elseif(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/cmake/LogosModule.cmake")
    include(cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT.")
endif()

# Derive the module name from metadata.json — single source of truth.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/metadata.json" METADATA_JSON)
string(JSON MODULE_NAME GET ${METADATA_JSON} name)

# Universal core module: we write only the impl class; the Qt plugin glue is
# generated from src/amm_module_impl.h (because metadata.json sets
# "interface": "universal"). EXTERNAL_LIBS links amm_ffi — the pure-Rust,
# transport-independent AMM brain (PDA/decode/quote/plan/encode) exposed as a
# JSON FFI (amm_ffi.h), the same library the UI links. The chain I/O
# dependency (lez_core) is declared in metadata.json and reached via
# modules().lez_core in the impl, so it is NOT listed here.
logos_module(
    NAME ${MODULE_NAME}
    SOURCES
        src/amm_module_impl.h
        src/amm_module_impl.cpp
    EXTERNAL_LIBS
        amm_ffi
)
