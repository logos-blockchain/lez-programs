cmake_minimum_required(VERSION 3.21)
project(AmmPortfolioSyncTest LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
find_package(Qt6 6.8 REQUIRED COMPONENTS Core Network Test)
qt_standard_project_setup(REQUIRES 6.8)
get_filename_component(REPO "${CMAKE_CURRENT_SOURCE_DIR}/../../../.." ABSOLUTE)
include(CTest)
set(wallet "${REPO}/apps/shared/wallet")
set(backend_source "${REPO}/apps/amm/src/AmmUiBackend.cpp")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${backend_source}")
file(READ "${backend_source}" backend)
string(FIND "${backend}" "void AmmUiBackend::refreshPortfolio()" begin)
string(FIND "${backend}" "QVariantMap AmmUiBackend::configAccount()" end)
if(begin LESS 0 OR end LESS begin)
    message(FATAL_ERROR "Portfolio methods not found in AmmUiBackend.cpp")
endif()
math(EXPR length "${end} - ${begin}")
string(SUBSTRING "${backend}" ${begin} ${length} methods)
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/portfolio-methods.inc" "${methods}")
qt_add_executable(amm_portfolio_sync_test
    AmmPortfolioSyncTest.cpp
    "${wallet}/src/WalletProvider.cpp"
    "${wallet}/src/WalletController.cpp"
    "${wallet}/src/WalletController.h"
    "${wallet}/src/WalletAccountModel.cpp"
    "${wallet}/src/WalletAccountModel.h"
    "${wallet}/src/WalletPortfolioService.cpp"
)
target_include_directories(amm_portfolio_sync_test PRIVATE
    "${CMAKE_CURRENT_BINARY_DIR}"
    "${wallet}/src"
    "${wallet}/tests/support"
)
target_link_libraries(amm_portfolio_sync_test PRIVATE Qt6::Core Qt6::Network Qt6::Test)
add_test(NAME amm_portfolio_sync COMMAND amm_portfolio_sync_test)
