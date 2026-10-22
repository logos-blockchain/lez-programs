cmake_minimum_required(VERSION 3.21)

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    project(RegistryLoaderTest LANGUAGES CXX)
    find_package(Qt6 6.4 REQUIRED COMPONENTS Core Network Test)
    set(CMAKE_AUTOMOC ON)
    include(CTest)
else()
    find_package(Qt6 6.8 REQUIRED COMPONENTS Test)
endif()

qt_add_executable(registry_loader_test
    RegistryLoaderTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/../../src/RegistryLoader.cpp
)
target_include_directories(registry_loader_test PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)
target_link_libraries(registry_loader_test PRIVATE
    Qt6::Core
    Qt6::Network
    Qt6::Test
)
add_test(NAME registry_loader COMMAND registry_loader_test)
