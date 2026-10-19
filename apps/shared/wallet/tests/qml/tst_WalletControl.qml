import QtQuick
import QtTest
import Logos.Wallet as Wallet

Item {
    id: root
    width: 800
    height: 600

    Component {
        id: backendComponent

        QtObject {
            property bool isWalletOpen: false
            property bool walletExists: true
            property bool completeOpenImmediately: true
            property bool createWalletFails: false
            property bool createWalletRefreshFails: false
            property bool accountRefreshFails: false
            property string walletHome: "/wallet"
            property string walletSyncStatus: "closed"
            property string walletSyncError: ""
            property bool deferOpen: false
            property int openCalls: 0
            property int createCalls: 0
            property int publicAccountCalls: 0
            property int privateAccountCalls: 0
            property int disconnectCalls: 0
            property int primaryAccountCalls: 0
            property int aliasCalls: 0
            property string primaryAccountAddress: ""
            property string primaryAccountName: ""
            property string activeNetwork: "testnet"
            property string networkStatus: "ready"
            property string assetStatus: "ready"
            property string assetError: ""
            property var assets: []

            function openExisting() {
                openCalls++
                if (deferOpen) {
                    walletSyncStatus = "opening"
                } else {
                    isWalletOpen = true
                    walletSyncStatus = "ready"
                }
                return true
            }

            function createNewDefault(_password) {
                createCalls++
                if (createWalletFails)
                    return ""
                isWalletOpen = true
                walletSyncStatus = createWalletRefreshFails ? "error" : "ready"
                walletSyncError = createWalletRefreshFails ? "read_failed" : ""
                return "alpha beta gamma"
            }

            function createAccountPublic() {
                publicAccountCalls++
                if (accountRefreshFails) {
                    walletSyncStatus = "error"
                    walletSyncError = "read_failed"
                }
                return "a".repeat(64)
            }

            function createAccountPrivate() {
                privateAccountCalls++
                if (accountRefreshFails) {
                    walletSyncStatus = "error"
                    walletSyncError = "read_failed"
                }
                return "b".repeat(64)
            }

            function disconnectWallet() {
                disconnectCalls++
                isWalletOpen = false
            }

            function setPrimaryAccount(address) {
                primaryAccountCalls++
                primaryAccountAddress = address
                return true
            }

            function setAccountAlias(_address, _alias) {
                aliasCalls++
                return true
            }
        }
    }

    Component {
        id: modelComponent
        ListModel { }
    }

    Component {
        id: portfolioComponent

        QtObject {
            property string assetStatus: "ready"
            property string assetError: ""
            property var assets: []
        }
    }

    Component {
        id: networkComponent

        QtObject {
            property string activeNetwork: ""
            property string networkStatus: "ready"
        }
    }

    Component {
        id: controlComponent
        Wallet.WalletControl {
            width: 320
            height: implicitHeight
            viewportWidth: 800
        }
    }

    Component {
        id: compactWindowComponent

        Window {
            width: 360
            height: 240
            visible: true

            property alias control: walletControl

            Wallet.WalletControl {
                id: walletControl
                x: 20
                y: 8
                width: 320
                height: implicitHeight
                viewportWidth: parent.width
            }
        }
    }

    Component {
        id: compactDialogWindowComponent

        Window {
            width: 360
            height: 600
            visible: true

            property alias control: walletControl

            Wallet.WalletControl {
                id: walletControl
                x: parent.width - width - 12
                y: 8
                width: 40
                height: implicitHeight
                viewportWidth: parent.width
            }
        }
    }

    TestCase {
        name: "WalletControl"
        when: windowShown

        function createControl(walletProperties, accounts) {
            const backend = createTemporaryObject(backendComponent, root, walletProperties || {})
            verify(backend, "Backend exists")
            const model = createTemporaryObject(modelComponent, root)
            verify(model, "Account model exists")
            for (const account of accounts || [])
                model.append(accountData(account))
            const control = createTemporaryObject(controlComponent, root, {
                wallet: backend,
                accountModel: model
            })
            verify(control, "Wallet control exists")
            return { backend, model, control }
        }

        function accountData(account) {
            return {
                name: account.name || "Account",
                alias: account.alias || "",
                address: account.address || "",
                displayAddress: account.displayAddress || account.address || "",
                balance: account.balance || "0",
                isPublic: account.isPublic === true,
                kind: account.kind || (account.isPublic === false ? "private" : "user"),
                section: account.section || "accounts",
                programName: account.programName || "",
                accountType: account.accountType || "",
                decodedData: account.decodedData || "",
                visibility: account.visibility || (account.isPublic === false ? "private" : "public"),
                canBePrimary: account.canBePrimary === undefined ? true : account.canBePrimary,
                isPrimary: account.isPrimary === true
            }
        }

        function test_opensExistingWallet() {
            const fixture = createControl({ walletExists: true }, [])
            const connectButton = findChild(fixture.control, "walletConnectButton")
            verify(connectButton, "Connect button exists")
            mouseClick(connectButton)
            compare(fixture.backend.openCalls, 1)
            tryCompare(fixture.control, "connected", true)
        }

        function test_surfacesDeferredOpenFailure() {
            const fixture = createControl({ walletExists: true, deferOpen: true }, [])
            mouseClick(findChild(fixture.control, "walletConnectButton"))
            compare(fixture.backend.openCalls, 1)
            compare(fixture.control.syncStatus, "opening")

            fixture.backend.walletSyncStatus = "error"
            fixture.backend.walletSyncError = "open_failed"
            const dialog = findChild(fixture.control, "walletMessageDialog")
            tryCompare(dialog, "opened", true)
            verify(dialog.message.includes("open_failed"))
        }

        function test_requiresSeedBackupAcknowledgement() {
            const fixture = createControl({ walletExists: false }, [])
            mouseClick(findChild(fixture.control, "walletConnectButton"))

            const dialog = findChild(fixture.control, "createWalletDialog")
            tryCompare(dialog, "opened", true)
            const password = findChild(dialog, "walletPasswordField")
            const confirmation = findChild(dialog, "walletConfirmPasswordField")
            const createButton = findChild(dialog, "createWalletButton")
            verify(password && confirmation && createButton, "Wallet fields exist")

            password.text = "secret"
            confirmation.text = "different"
            mouseClick(createButton)
            compare(fixture.backend.createCalls, 0)
            verify(dialog.errorText.length > 0)

            confirmation.text = "secret"
            mouseClick(createButton)
            compare(fixture.backend.createCalls, 1)
            compare(dialog.mnemonic, "alpha beta gamma")

            const acknowledgement = findChild(dialog, "walletBackupAcknowledgement")
            const continueButton = findChild(dialog, "walletContinueButton")
            verify(acknowledgement && continueButton, "Backup controls exist")
            verify(!continueButton.enabled)
            mouseClick(acknowledgement)
            verify(continueButton.enabled)
            mouseClick(continueButton)
            tryCompare(dialog, "opened", false)
        }

        function test_showsWalletCreationFailure() {
            const fixture = createControl({ walletExists: false, createWalletFails: true }, [])
            mouseClick(findChild(fixture.control, "walletConnectButton"))
            const dialog = findChild(fixture.control, "createWalletDialog")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "walletPasswordField").text = "secret"
            findChild(dialog, "walletConfirmPasswordField").text = "secret"
            findChild(dialog, "createWalletButton").clicked()
            compare(fixture.backend.createCalls, 1)
            compare(dialog.mnemonic, "")
            compare(dialog.errorText, "Wallet could not be created.")
            verify(dialog.opened)
        }

        function test_warnsWhenCreatedWalletCannotRefresh() {
            const fixture = createControl({
                walletExists: false,
                createWalletRefreshFails: true
            }, [])
            mouseClick(findChild(fixture.control, "walletConnectButton"))
            const dialog = findChild(fixture.control, "createWalletDialog")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "walletPasswordField").text = "secret"
            findChild(dialog, "walletConfirmPasswordField").text = "secret"
            findChild(dialog, "createWalletButton").clicked()
            tryCompare(dialog, "mnemonic", "alpha beta gamma")
            const message = findChild(fixture.control, "walletMessageDialog")
            verify(!message.opened)
            mouseClick(findChild(dialog, "walletBackupAcknowledgement"))
            mouseClick(findChild(dialog, "walletContinueButton"))

            tryCompare(message, "opened", true)
            compare(message.message,
                    "Wallet was created, but could not be refreshed. Reconnect the wallet to refresh it.")
        }

        function test_clampsSelectionAndDisconnectsLocally() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true },
                { name: "Two", address: "b".repeat(64), balance: "20", isPublic: false }
            ])
            fixture.control.selectedIndex = 1
            compare(fixture.control.selectedAddress, "b".repeat(64))
            fixture.model.clear()
            tryCompare(fixture.control, "selectedIndex", -1)
            compare(fixture.control.selectedAddress, "")

            fixture.model.append(accountData({
                name: "One", address: "a".repeat(64), balance: "10", isPublic: true
            }))
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const disconnectButton = findChild(fixture.control, "walletDisconnectButton")
            tryVerify(function() { return disconnectButton.visible })
            mouseClick(disconnectButton)
            compare(fixture.backend.disconnectCalls, 1)
            tryCompare(fixture.control, "connected", false)
        }

        function test_waitsForPrimaryDelegateBeforeShowingAccountType() {
            const address = "a".repeat(64)
            const fixture = createControl({
                isWalletOpen: true,
                primaryAccountAddress: address,
                primaryAccountName: "Primary"
            }, [])
            mouseClick(findChild(fixture.control, "walletAccountButton"))

            const accountType = findChild(fixture.control, "walletPrimaryAccountType")
            verify(accountType, "Primary account type exists")
            verify(!accountType.visible, "Account type waits for its selected delegate")

            fixture.model.append(accountData({
                name: "Primary",
                address: address,
                balance: "10",
                isPublic: true,
                isPrimary: true
            }))
            tryCompare(fixture.control, "selectedAddress", address)
            tryCompare(accountType, "visible", true)
            compare(accountType.text, "Public user account")
        }

        function test_connectedButtonClosesOpenMenu() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            const accountButton = findChild(fixture.control, "walletAccountButton")
            const menu = findChild(fixture.control, "walletMenu")

            mouseClick(accountButton)
            tryCompare(menu, "opened", true)
            mouseClick(accountButton)
            tryCompare(menu, "opened", false)
        }

        function test_walletMenuClosesWithEscape() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            const accountButton = findChild(fixture.control, "walletAccountButton")
            const menu = findChild(fixture.control, "walletMenu")

            mouseClick(accountButton)
            tryCompare(menu, "opened", true)
            keyClick(Qt.Key_Escape)
            tryCompare(menu, "opened", false)
        }

        function test_createAccountDialogOwnsKeyboardFocus() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            mouseClick(findChild(fixture.control, "walletAccountsButton"))

            const backButton = findChild(fixture.control, "walletAccountsBackButton")
            const addButton = findChild(fixture.control, "walletAddAccountButton")
            verify(backButton && addButton, "Account controls exist")
            mouseClick(addButton)

            const dialog = findChild(fixture.control, "createAccountDialog")
            const privateSwitch = findChild(dialog, "privateAccountSwitch")
            tryCompare(dialog, "opened", true)
            tryVerify(function() { return privateSwitch.activeFocus })
            for (let index = 0; index < 6; ++index) {
                keyClick(Qt.Key_Tab)
                verify(!backButton.activeFocus, "Focus remains inside the dialog")
            }
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
        }

        function test_walletMessageDialogClosesWithEscape() {
            const fixture = createControl({ isWalletOpen: true }, [])
            const dialog = findChild(fixture.control, "walletMessageDialog")

            dialog.open()
            tryCompare(dialog, "opened", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
        }

        function test_openMenuTracksControlMovement() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            fixture.control.x = 20
            fixture.control.y = 20
            const accountButton = findChild(fixture.control, "walletAccountButton")
            const menu = findChild(fixture.control, "walletMenu")

            mouseClick(accountButton)
            tryCompare(menu, "opened", true)
            const initialMenu = menu.contentItem.mapToItem(root, 0, 0)
            const initialButton = accountButton.mapToItem(root, 0, 0)

            fixture.control.x += 300
            fixture.control.y += 30
            wait(0)

            const movedMenu = menu.contentItem.mapToItem(root, 0, 0)
            const movedButton = accountButton.mapToItem(root, 0, 0)
            compare(movedMenu.x - movedButton.x, initialMenu.x - initialButton.x)
            compare(movedMenu.y - movedButton.y, initialMenu.y - initialButton.y)
        }

        function test_selectsAccount() {
            const fixture = createControl({ isWalletOpen: true }, [
                {
                    name: "One",
                    address: "a".repeat(64),
                    displayAddress: "base58-one",
                    balance: "10",
                    isPublic: true
                },
                {
                    name: "Two",
                    address: "b".repeat(64),
                    displayAddress: "base58-two",
                    balance: "20",
                    isPublic: false
                }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const accountsButton = findChild(fixture.control, "walletAccountsButton")
            tryVerify(function() { return accountsButton.visible })
            mouseClick(accountsButton)

            const accountList = findChild(fixture.control, "walletAccountList")
            tryCompare(accountList, "count", 2)
            tryVerify(function() { return accountList.itemAtIndex(1) !== null })
            const secondAccount = accountList.itemAtIndex(1)
            mouseClick(secondAccount)
            tryCompare(fixture.control, "selectedIndex", 1)
            compare(fixture.backend.primaryAccountAddress, "b".repeat(64))
            compare(fixture.control.selectedAddress, "b".repeat(64))
            compare(fixture.control.selectedDisplayAddress, "base58-two")
        }

        function test_flatWalletActionsUseReadableForeground() {
            const fixture = createControl({
                isWalletOpen: true,
                assets: [{
                    name: "Available",
                    balance: "0",
                    definitionId: "c".repeat(64),
                    displayDefinitionId: "base58-available",
                    status: "ready",
                    section: "available"
                }]
            }, [
                {
                    name: "One",
                    address: "a".repeat(64),
                    balance: "10",
                    isPublic: true,
                    isPrimary: true
                },
                {
                    name: "Two",
                    address: "b".repeat(64),
                    balance: "20",
                    isPublic: true
                }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))

            const available = findChild(fixture.control, "walletAvailableAssetsButton")
            tryVerify(function() { return available && available.visible })
            compare(available.flat, true)
            compare(available.palette.windowText, "#d4d4d8")
            compare(available.contentItem.color, "#d4d4d8")
            mouseClick(available)
            tryCompare(fixture.control, "availableExpanded", true)

            mouseClick(findChild(fixture.control, "walletAccountsButton"))
            const advanced = findChild(fixture.control, "walletAdvancedAccountsButton")
            tryVerify(function() { return advanced && advanced.visible })
            compare(advanced.flat, true)
            compare(advanced.palette.windowText, "#d4d4d8")
            compare(advanced.contentItem.color, "#d4d4d8")

            const accountList = findChild(fixture.control, "walletAccountList")
            tryVerify(function() { return accountList.itemAtIndex(1) !== null })
            const secondAccount = accountList.itemAtIndex(1)
            const rename = findChild(secondAccount, "walletRenameButton")
            const makePrimary = findChild(secondAccount, "walletMakePrimaryButton")
            verify(rename && makePrimary, "Account action buttons exist")
            for (const action of [rename, makePrimary]) {
                compare(action.flat, true)
                compare(action.palette.windowText, "#d4d4d8")
                compare(action.contentItem.color, "#d4d4d8")
            }
        }

        function test_tokenAssetsRenderInBoxes() {
            const fixture = createControl({
                isWalletOpen: true,
                assets: [
                    {
                        name: "Held token",
                        balance: "42",
                        definitionId: "c".repeat(64),
                        displayDefinitionId: "base58-held-token",
                        status: "ready",
                        section: "assets"
                    },
                    {
                        name: "Available token",
                        balance: "0",
                        definitionId: "d".repeat(64),
                        displayDefinitionId: "base58-available-token",
                        status: "ready",
                        section: "available"
                    }
                ]
            }, [])
            mouseClick(findChild(fixture.control, "walletAccountButton"))

            const heldRepeater = findChild(fixture.control, "walletAssetRepeater")
            verify(heldRepeater, "Held token repeater exists")
            let held = null
            tryVerify(function() {
                held = heldRepeater.itemAt(0)
                return held !== null
            })
            verify(held, "Held token box exists")
            tryCompare(held, "visible", true)
            compare(held.implicitHeight, 68)
            compare(held.radius, 10)
            compare(held.border.width, 1)
            compare(held.border.color, "#3f3f46")

            const availableRepeater = findChild(fixture.control, "walletAvailableAssetRepeater")
            verify(availableRepeater, "Available token repeater exists")
            let available = null
            tryVerify(function() {
                available = availableRepeater.itemAt(1)
                return available !== null
            })
            verify(available, "Available token box exists")
            compare(available.visible, false)
            mouseClick(findChild(fixture.control, "walletAvailableAssetsButton"))
            tryCompare(available, "visible", true)
            compare(available.implicitHeight, 64)
            compare(available.radius, 10)
            compare(available.border.width, 1)
            compare(available.border.color, "#3f3f46")
        }

        function test_usesExplicitPortfolioAndNetworkProviders() {
            const fixture = createControl({
                isWalletOpen: true,
                activeNetwork: "wallet network",
                networkStatus: "error",
                assetStatus: "ready",
                assets: [{
                    name: "Wallet available token",
                    balance: "0",
                    definitionId: "a".repeat(64),
                    status: "ready",
                    section: "available"
                }]
            }, [])
            const portfolio = createTemporaryObject(portfolioComponent, root, {
                assetStatus: "loading",
                assets: [{
                    name: "Portfolio token",
                    balance: "42",
                    definitionId: "b".repeat(64),
                    status: "ready",
                    section: "assets"
                }]
            })
            const network = createTemporaryObject(networkComponent, root, {
                activeNetwork: "shared testnet",
                networkStatus: "loading"
            })
            verify(portfolio && network, "Shared providers exist")

            fixture.control.portfolio = portfolio
            fixture.control.network = network

            compare(fixture.control.portfolioProvider, portfolio)
            compare(fixture.control.networkProvider, network)
            compare(fixture.control.walletAssets[0].name, "Portfolio token")
            compare(fixture.control.assetStatus, "loading")
            compare(fixture.control.activeNetwork, "shared testnet")
            compare(fixture.control.networkStatus, "loading")

            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const indicator = findChild(fixture.control, "walletNetworkStatusIndicator")
            const networkName = findChild(fixture.control, "walletNetworkName")
            const loading = findChild(fixture.control, "walletAssetsLoadingLabel")
            const heldAssets = findChild(fixture.control, "walletAssetRepeater")
            verify(indicator && networkName && loading && heldAssets, "Provider UI exists")
            tryCompare(indicator, "color", "#f59e0b")
            tryCompare(networkName, "text", "shared testnet")
            tryCompare(loading, "visible", true)
            tryCompare(heldAssets, "count", 1)
            tryCompare(heldAssets.itemAt(0), "visible", true)
        }

        function test_accountNavigationKeepsOverviewInsidePopup() {
            const assets = []
            for (let index = 0; index < 10; ++index) {
                assets.push({
                    name: "Token " + index,
                    balance: "100",
                    definitionId: "c".repeat(64),
                    displayDefinitionId: "base58-token-" + index,
                    status: "ready",
                    section: "assets"
                })
            }
            const fixture = createControl({ isWalletOpen: true, assets: assets }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const stack = findChild(fixture.control, "walletStack")
            verify(stack, "Wallet stack exists")
            verify(stack.clip, "Wallet pages are clipped to the popup")
            mouseClick(findChild(fixture.control, "walletAccountsButton"))
            tryCompare(stack, "busy", false)
            compare(stack.depth, 2)

            mouseClick(findChild(fixture.control, "walletAccountsBackButton"))
            tryCompare(stack, "busy", false)
            compare(stack.depth, 1)
            compare(stack.currentItem.x, 0)
            const overviewContent = findChild(fixture.control, "walletOverviewContent")
            verify(overviewContent, "Wallet overview content exists")
            compare(overviewContent.mapToItem(stack, 0, 0).x, 0)
        }

        function test_programRecordCannotBecomePrimary() {
            const userAddress = "a".repeat(64)
            const programAddress = "c".repeat(64)
            const fixture = createControl({
                isWalletOpen: true,
                primaryAccountAddress: userAddress,
                primaryAccountName: "Trading"
            }, [
                {
                    name: "Trading",
                    address: userAddress,
                    balance: "10",
                    isPublic: true,
                    kind: "user",
                    isPrimary: true
                },
                {
                    name: "Token definition",
                    address: programAddress,
                    balance: "0",
                    isPublic: true,
                    kind: "token_definition",
                    section: "advanced",
                    programName: "Token",
                    accountType: "TokenDefinition",
                    canBePrimary: false
                }
            ])
            compare(fixture.control.selectedAddress, userAddress)
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            mouseClick(findChild(fixture.control, "walletAccountsButton"))
            mouseClick(findChild(fixture.control, "walletAdvancedAccountsButton"))
            const list = findChild(fixture.control, "walletAccountList")
            tryVerify(function() { return list.itemAtIndex(1) !== null })
            mouseClick(list.itemAtIndex(1))
            compare(fixture.backend.primaryAccountCalls, 0)
            compare(fixture.control.selectedAddress, userAddress)
        }

        function test_advancedShowsProgramAndDecodedData() {
            const decodedData = "{\n  \"name\": \"Test token\"\n}"
            const fixture = createControl({ isWalletOpen: true }, [
                {
                    name: "Token definition",
                    address: "c".repeat(64),
                    balance: "0",
                    isPublic: true,
                    kind: "token_definition",
                    section: "advanced",
                    programName: "Token",
                    accountType: "TokenDefinition",
                    decodedData: decodedData,
                    canBePrimary: false
                }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            mouseClick(findChild(fixture.control, "walletAccountsButton"))
            mouseClick(findChild(fixture.control, "walletAdvancedAccountsButton"))

            const list = findChild(fixture.control, "walletAccountList")
            tryVerify(function() { return list.itemAtIndex(0) !== null })
            const program = findChild(list.itemAtIndex(0), "walletProgramName")
            const decoded = findChild(list.itemAtIndex(0), "walletDecodedData")
            verify(program && decoded, "Advanced details exist")
            tryCompare(program, "visible", true)
            compare(program.text, "Program: Token")
            tryCompare(decoded, "visible", true)
            compare(decoded.text, decodedData)
        }

        function test_onlyProgramRecordsLeavesPrimaryEmpty() {
            const fixture = createControl({ isWalletOpen: true }, [{
                name: "Token definition",
                address: "c".repeat(64),
                balance: "0",
                isPublic: true,
                kind: "token_definition",
                section: "advanced",
                canBePrimary: false
            }])
            compare(fixture.control.selectedIndex, -1)
            compare(fixture.control.selectedAddress, "")
            compare(fixture.control.primaryName, "")
        }

        function test_createsAccount() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const accountsButton = findChild(fixture.control, "walletAccountsButton")
            tryVerify(function() { return accountsButton.visible })
            mouseClick(accountsButton)

            const addButton = findChild(fixture.control, "walletAddAccountButton")
            tryVerify(function() { return addButton.visible })
            addButton.clicked()
            const dialog = findChild(fixture.control, "createAccountDialog")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "createAccountButton").clicked()
            compare(fixture.backend.publicAccountCalls, 1)
            tryCompare(dialog, "opened", false)
        }

        function test_createsPrivateAccount() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            mouseClick(findChild(fixture.control, "walletAccountsButton"))
            const addButton = findChild(fixture.control, "walletAddAccountButton")
            tryVerify(function() { return addButton.visible })
            addButton.clicked()
            const dialog = findChild(fixture.control, "createAccountDialog")
            tryCompare(dialog, "opened", true)
            mouseClick(findChild(dialog, "privateAccountSwitch"))
            findChild(dialog, "createAccountButton").clicked()
            compare(fixture.backend.privateAccountCalls, 1)
            tryCompare(dialog, "opened", false)
        }

        function test_warnsWhenCreatedAccountCannotRefresh() {
            const fixture = createControl({
                isWalletOpen: true,
                walletSyncStatus: "ready",
                accountRefreshFails: true
            }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            mouseClick(findChild(fixture.control, "walletAccountsButton"))
            const addButton = findChild(fixture.control, "walletAddAccountButton")
            tryVerify(function() { return addButton.visible })
            addButton.clicked()
            const dialog = findChild(fixture.control, "createAccountDialog")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "createAccountButton").clicked()
            tryCompare(dialog, "opened", false)

            const message = findChild(fixture.control, "walletMessageDialog")
            tryCompare(message, "opened", true)
            compare(message.message,
                    "Account was created, but could not be refreshed. Reconnect the wallet to refresh it.")
        }

        function test_compactLayoutHasStableWidth() {
            const fixture = createControl({ isWalletOpen: false }, [])
            fixture.control.viewportWidth = 480
            verify(fixture.control.compactLayout)
            compare(fixture.control.implicitWidth, 40)
            fixture.control.viewportWidth = 900
            verify(!fixture.control.compactLayout)
            compare(fixture.control.implicitWidth, 108)
        }

        function test_walletDialogsUseWindowViewport() {
            const window = createTemporaryObject(compactDialogWindowComponent, root)
            verify(window, "Compact window exists")
            waitForRendering(window.contentItem)

            const dialogNames = [
                "createWalletDialog",
                "createAccountDialog",
                "walletMessageDialog"
            ]
            for (const name of dialogNames) {
                const dialog = findChild(window.control, name)
                verify(dialog, name + " exists")
                dialog.open()
                tryCompare(dialog, "opened", true)
                compare(dialog.width, window.width - 32)
                verify(dialog.x >= 0)
                verify(dialog.x + dialog.width <= window.width)
                dialog.close()
                tryCompare(dialog, "opened", false)
            }
        }

        function test_accountsRemainReachableInShortWindow() {
            const backend = createTemporaryObject(backendComponent, root, { isWalletOpen: true })
            const model = createTemporaryObject(modelComponent, root)
            verify(backend && model, "Wallet fixture exists")
            for (let index = 0; index < 10; ++index) {
                model.append(accountData({
                    name: "Account " + index,
                    address: String(index).repeat(64),
                    balance: String(index),
                    isPublic: true
                }))
            }

            const window = createTemporaryObject(compactWindowComponent, root)
            verify(window, "Short window exists")
            window.control.wallet = backend
            window.control.accountModel = model
            waitForRendering(window.contentItem)

            mouseClick(findChild(window.control, "walletAccountButton"))
            const menu = findChild(window.control, "walletMenu")
            tryCompare(menu, "opened", true)
            mouseClick(findChild(window.control, "walletAccountsButton"))

            const accountList = findChild(window.control, "walletAccountList")
            const addButton = findChild(window.control, "walletAddAccountButton")
            tryCompare(accountList, "count", 10)
            verify(menu.y >= 12, "Menu top: " + menu.y)
            verify(menu.y + menu.height <= window.height - 12,
                   "Menu bottom: " + (menu.y + menu.height)
                   + ", window: " + window.height)
            verify(accountList.height > 0)
            verify(accountList.contentHeight > accountList.height)
            verify(addButton && addButton.visible, "Add account button is visible")
            const addPosition = addButton.mapToItem(window.contentItem, 0, 0)
            verify(addPosition.y >= menu.y)
            verify(addPosition.y + addButton.height <= menu.y + menu.height,
                   "Add account bottom: " + (addPosition.y + addButton.height)
                   + ", menu bottom: " + (menu.y + menu.height))
        }
    }
}
