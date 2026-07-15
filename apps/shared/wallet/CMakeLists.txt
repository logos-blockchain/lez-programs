cmake_minimum_required(VERSION 3.21)

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    project(LogosWallet LANGUAGES CXX)
    include(CTest)
endif()

option(LOGOS_WALLET_BUILD_QML "Build the Logos.Wallet QML module" ON)
option(LOGOS_WALLET_BUILD_ACCESS "Build the generated-SDK wallet adapter" ON)
set(LOGOS_WALLET_GENERATED_DIR "" CACHE PATH "Path to generated Logos SDK sources")

if(LOGOS_WALLET_BUILD_ACCESS
   AND NOT EXISTS "${LOGOS_WALLET_GENERATED_DIR}/logos_sdk.h"
   AND NOT EXISTS "${LOGOS_WALLET_GENERATED_DIR}/include/logos_sdk.h")
    message(FATAL_ERROR
        "logos_wallet_access requires logos_sdk.h; set LOGOS_WALLET_GENERATED_DIR"
    )
endif()

find_package(Qt6 6.8 REQUIRED COMPONENTS Core)
if(LOGOS_WALLET_BUILD_ACCESS OR BUILD_TESTING)
    find_package(Qt6 6.8 REQUIRED COMPONENTS Network)
endif()
set(CMAKE_AUTOMOC ON)

if(LOGOS_WALLET_BUILD_ACCESS)
    add_library(logos_wallet_access STATIC
        src/WalletProvider.h
        src/WalletProvider.cpp
        src/LogosWalletProvider.h
        src/LogosWalletProvider.cpp
        src/WalletAccountModel.h
        src/WalletAccountModel.cpp
        src/WalletController.h
        src/WalletController.cpp
    )
    set_target_properties(logos_wallet_access PROPERTIES
        AUTOMOC ON
        POSITION_INDEPENDENT_CODE ON
    )
    target_compile_features(logos_wallet_access PUBLIC cxx_std_17)
    target_include_directories(logos_wallet_access
        PUBLIC
            "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/src>"
        PRIVATE
            "${LOGOS_WALLET_GENERATED_DIR}"
            "${LOGOS_WALLET_GENERATED_DIR}/include"
    )
    target_link_libraries(logos_wallet_access
        PUBLIC Qt6::Core
        PRIVATE Qt6::Network
    )
endif()

if(LOGOS_WALLET_BUILD_QML)
    find_package(Qt6 6.8 REQUIRED COMPONENTS Qml Quick QuickControls2)
    qt_policy(SET QTP0004 NEW)

    set(wallet_qml_output_dir "${CMAKE_CURRENT_BINARY_DIR}/qml/Logos/Wallet")
    set(wallet_internal_qml
        qml/internal/WalletIconButton.qml
        qml/internal/CopyButton.qml
        qml/internal/AccountDelegate.qml
        qml/internal/CreateAccountDialog.qml
        qml/internal/CreateWalletDialog.qml
        qml/internal/WalletMessageDialog.qml
    )
    set(wallet_public_qml
        qml/WalletControl.qml
        qml/TransactionConfirmationDialog.qml
        qml/SubmittedTransaction.qml
    )
    set(wallet_icons
        qml/internal/icons/account.svg
        qml/internal/icons/back.svg
        qml/internal/icons/checkmark.svg
        qml/internal/icons/copy.svg
        qml/internal/icons/power.svg
    )
    foreach(qml_file IN LISTS wallet_public_qml wallet_internal_qml)
        get_filename_component(qml_name "${qml_file}" NAME)
        set_source_files_properties("${qml_file}" PROPERTIES QT_RESOURCE_ALIAS "${qml_name}")
    endforeach()
    foreach(icon IN LISTS wallet_icons)
        get_filename_component(icon_name "${icon}" NAME)
        set_source_files_properties("${icon}" PROPERTIES QT_RESOURCE_ALIAS "icons/${icon_name}")
    endforeach()
    set_source_files_properties(${wallet_internal_qml} PROPERTIES QT_QML_INTERNAL_TYPE TRUE)

    qt_add_library(logos_wallet_qml SHARED)
    qt_add_qml_module(logos_wallet_qml
        URI Logos.Wallet
        VERSION 1.0
        RESOURCE_PREFIX /qt/qml
        OUTPUT_DIRECTORY "${wallet_qml_output_dir}"
        TYPEINFO plugins.qmltypes
        QML_FILES
            ${wallet_public_qml}
            ${wallet_internal_qml}
        RESOURCES
            ${wallet_icons}
    )
    target_link_libraries(logos_wallet_qml PRIVATE
        Qt6::Core
        Qt6::Qml
        Qt6::Quick
        Qt6::QuickControls2
    )
    set_target_properties(logos_wallet_qml logos_wallet_qmlplugin PROPERTIES
        LIBRARY_OUTPUT_DIRECTORY "${wallet_qml_output_dir}"
        RUNTIME_OUTPUT_DIRECTORY "${wallet_qml_output_dir}"
        BUILD_WITH_INSTALL_RPATH ON
        INSTALL_RPATH "$ORIGIN"
    )

    set(wallet_qml_install_dir "lib/qml/Logos/Wallet")
    install(TARGETS logos_wallet_qml logos_wallet_qmlplugin
        LIBRARY DESTINATION "${wallet_qml_install_dir}"
        RUNTIME DESTINATION "${wallet_qml_install_dir}"
    )
    install(FILES
        "${wallet_qml_output_dir}/qmldir"
        "${wallet_qml_output_dir}/plugins.qmltypes"
        DESTINATION "${wallet_qml_install_dir}"
        OPTIONAL
    )
endif()

if(BUILD_TESTING)
    find_package(Qt6 6.8 REQUIRED COMPONENTS Test)

    add_executable(logos_wallet_access_test
        tests/cpp/LogosWalletProviderTest.cpp
        src/WalletProvider.cpp
        src/LogosWalletProvider.cpp
        src/WalletAccountModel.cpp
        src/WalletAccountModel.h
        src/WalletController.cpp
        src/WalletController.h
    )
    set_target_properties(logos_wallet_access_test PROPERTIES AUTOMOC ON)
    target_compile_features(logos_wallet_access_test PRIVATE cxx_std_17)
    target_include_directories(logos_wallet_access_test PRIVATE
        tests/cpp/fixtures
        tests/support
        src
    )
    target_link_libraries(logos_wallet_access_test PRIVATE
        Qt6::Core
        Qt6::Network
        Qt6::Test
    )
    add_test(NAME logos_wallet_access COMMAND logos_wallet_access_test)

    if(LOGOS_WALLET_BUILD_QML)
        find_package(Qt6 6.8 REQUIRED COMPONENTS QuickTest)
        add_executable(logos_wallet_qml_test tests/qml/main.cpp)
        target_compile_definitions(logos_wallet_qml_test PRIVATE
            QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/tests/qml"
        )
        target_link_libraries(logos_wallet_qml_test PRIVATE Qt6::QuickTest)
        add_dependencies(logos_wallet_qml_test logos_wallet_qmlplugin)
        add_test(NAME logos_wallet_qml COMMAND logos_wallet_qml_test)
        set_tests_properties(logos_wallet_qml PROPERTIES ENVIRONMENT
            "QT_QPA_PLATFORM=offscreen;QT_QUICK_BACKEND=software;QML2_IMPORT_PATH=${CMAKE_CURRENT_BINARY_DIR}/qml;QML_IMPORT_PATH=${CMAKE_CURRENT_BINARY_DIR}/qml"
        )
    endif()
endif()
