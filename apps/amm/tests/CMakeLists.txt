cmake_minimum_required(VERSION 3.21)

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    project(AmmUiTests LANGUAGES CXX)
    include(CTest)
endif()

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Network Qml Test)
qt_standard_project_setup(REQUIRES 6.8)

set(wallet_source_dir "${CMAKE_CURRENT_SOURCE_DIR}/../../shared/wallet")
qt_add_executable(amm_wallet_polling_test
    cpp/WalletPollingTest.cpp
    "${wallet_source_dir}/src/WalletProvider.cpp"
    "${wallet_source_dir}/src/WalletAccountModel.cpp"
    "${wallet_source_dir}/src/WalletAccountModel.h"
    "${wallet_source_dir}/src/WalletController.cpp"
    "${wallet_source_dir}/src/WalletController.h"
)
target_compile_features(amm_wallet_polling_test PRIVATE cxx_std_17)
target_compile_definitions(amm_wallet_polling_test PRIVATE
    AMM_QML_STATE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../qml/state"
)
target_include_directories(amm_wallet_polling_test PRIVATE
    "${wallet_source_dir}/src"
    "${wallet_source_dir}/tests/support"
)
target_link_libraries(amm_wallet_polling_test PRIVATE
    Qt6::Core
    Qt6::Network
    Qt6::Qml
    Qt6::Test
)
add_test(NAME amm_wallet_polling COMMAND amm_wallet_polling_test)
