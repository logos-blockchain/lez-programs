cmake_minimum_required(VERSION 3.21)

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    project(AmmSwapConfirmationTests LANGUAGES CXX)
    set(CMAKE_CXX_STANDARD 17)
    set(CMAKE_CXX_STANDARD_REQUIRED ON)
    include(CTest)
    find_package(Qt6 6.8 REQUIRED COMPONENTS Core Network Test)
    qt_standard_project_setup(REQUIRES 6.8)
else()
    find_package(Qt6 6.8 REQUIRED COMPONENTS Network Test)
endif()

qt_add_executable(amm_swap_confirmation_test
    tst_SwapConfirmation.cpp
    ../../src/SwapConfirmation.cpp
    ../../../shared/wallet/src/WalletController.cpp
    ../../../shared/wallet/src/WalletAccountModel.cpp
    ../../../shared/wallet/src/WalletProvider.cpp
)
target_compile_features(amm_swap_confirmation_test PRIVATE cxx_std_17)
target_include_directories(amm_swap_confirmation_test PRIVATE
    ../../src
    ../../../shared/wallet/src
    ../../../shared/wallet/tests/support
)
target_link_libraries(amm_swap_confirmation_test PRIVATE Qt6::Core Qt6::Network Qt6::Test)
add_test(NAME amm_swap_confirmation COMMAND amm_swap_confirmation_test)
