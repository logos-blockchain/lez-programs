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
            property string walletHome: "/wallet"
            property string syncStatus: "closed"
            property string syncError: ""
            property bool initialSync: false
            property bool syncProgressKnown: false
            property int syncCurrentBlock: 0
            property int syncTargetBlock: 0
            property int syncRemainingBlocks: 0
            property bool createLeavesSyncing: false
            property int openCalls: 0
            property int createCalls: 0
            property int publicAccountCalls: 0
            property int privateAccountCalls: 0
            property int cancelCalls: 0
            property int refreshCalls: 0
            property int disconnectCalls: 0

            function openExisting() {
                openCalls++
                isWalletOpen = true
                initialSync = false
                syncStatus = "ready"
                return true
            }

            function createNewDefault(_password) {
                createCalls++
                isWalletOpen = true
                initialSync = createLeavesSyncing
                syncStatus = createLeavesSyncing ? "syncing" : "ready"
                return "alpha beta gamma"
            }

            function createAccountPublic() {
                publicAccountCalls++
                return "a".repeat(64)
            }

            function createAccountPrivate() {
                privateAccountCalls++
                return "b".repeat(64)
            }

            function refreshAccounts() {
                refreshCalls++
                syncStatus = "syncing"
            }

            function disconnectWallet() {
                disconnectCalls++
                isWalletOpen = false
                initialSync = false
                syncStatus = "closed"
            }

            function cancelSync() {
                cancelCalls++
                syncError = "sync_cancelled"
                isWalletOpen = false
                initialSync = false
                syncStatus = "error"
            }
        }
    }

    Component {
        id: modelComponent
        ListModel { }
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
                model.append(account)
            const control = createTemporaryObject(controlComponent, root, {
                wallet: backend,
                accountModel: model
            })
            verify(control, "Wallet control exists")
            return { backend, model, control }
        }

        function test_opensExistingWallet() {
            const fixture = createControl({ walletExists: true }, [])
            const connectButton = findChild(fixture.control, "walletConnectButton")
            verify(connectButton, "Connect button exists")
            mouseClick(connectButton)
            compare(fixture.backend.openCalls, 1)
            tryCompare(fixture.control, "connected", true)
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

        function test_displaysAndCancelsSync() {
            const fixture = createControl({
                isWalletOpen: true,
                syncStatus: "syncing",
                initialSync: true,
                syncProgressKnown: true,
                syncCurrentBlock: 100,
                syncTargetBlock: 500,
                syncRemainingBlocks: 400
            }, [])
            const cancelButton = findChild(fixture.control, "walletCancelSyncButton")
            verify(cancelButton && cancelButton.visible, "Sync cancel button is visible")
            verify(fixture.control.syncProgressText.indexOf("100") >= 0)
            verify(fixture.control.syncProgressText.indexOf("500") >= 0)

            mouseClick(cancelButton)
            compare(fixture.backend.cancelCalls, 1)
            tryCompare(fixture.control, "synchronizing", false)
            tryCompare(fixture.control, "connected", false)
            const message = findChild(fixture.control, "walletMessageDialog")
            tryCompare(message, "opened", true)
            verify(message.message.indexOf("sync_cancelled") >= 0)
            message.close()

            const errorButton = findChild(fixture.control, "walletSyncErrorButton")
            verify(errorButton && errorButton.visible)
            mouseClick(errorButton)
            compare(fixture.backend.openCalls, 1)
            tryCompare(fixture.control, "syncFailed", false)
        }

        function test_creationShowsSyncProgressAndCancellation() {
            const fixture = createControl({
                walletExists: false,
                createLeavesSyncing: true,
                syncProgressKnown: true,
                syncCurrentBlock: 100,
                syncTargetBlock: 500,
                syncRemainingBlocks: 400
            }, [])
            mouseClick(findChild(fixture.control, "walletConnectButton"))
            const dialog = findChild(fixture.control, "createWalletDialog")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "walletPasswordField").text = "secret"
            findChild(dialog, "walletConfirmPasswordField").text = "secret"
            mouseClick(findChild(dialog, "createWalletButton"))
            tryCompare(dialog, "mnemonic", "alpha beta gamma")

            const status = findChild(dialog, "walletSyncStatus")
            const cancelButton = findChild(dialog, "walletCancelSyncButton")
            verify(status && status.visible, "Creation sync status is visible")
            verify(cancelButton && cancelButton.visible, "Creation sync cancel button is visible")
            verify(status.text.indexOf("100") >= 0)

            mouseClick(cancelButton)
            compare(fixture.backend.cancelCalls, 1)
            tryCompare(cancelButton, "visible", false)
            verify(status.visible)
            verify(status.text.indexOf("sync_cancelled") >= 0)
        }

        function test_clampsSelectionAndDisconnectsLocally() {
            const fixture = createControl({ isWalletOpen: true }, [
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true },
                { name: "Two", address: "b".repeat(64), balance: "20", isPublic: false }
            ])
            fixture.control.selectedIndex = 1
            compare(fixture.control.selectedAddress, "b".repeat(64))
            fixture.model.clear()
            tryCompare(fixture.control, "selectedIndex", 0)
            compare(fixture.control.selectedAddress, "")

            fixture.model.append({
                name: "One", address: "a".repeat(64), balance: "10", isPublic: true
            })
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const disconnectButton = findChild(fixture.control, "walletDisconnectButton")
            tryVerify(function() { return disconnectButton.visible })
            mouseClick(disconnectButton)
            compare(fixture.backend.disconnectCalls, 1)
            tryCompare(fixture.control, "connected", false)
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
                { name: "One", address: "a".repeat(64), balance: "10", isPublic: true },
                { name: "Two", address: "b".repeat(64), balance: "20", isPublic: false }
            ])
            mouseClick(findChild(fixture.control, "walletAccountButton"))
            const accountsButton = findChild(fixture.control, "walletAccountsButton")
            tryVerify(function() { return accountsButton.visible })
            mouseClick(accountsButton)

            const accountList = findChild(fixture.control, "walletAccountList")
            tryCompare(accountList, "count", 2)
            tryVerify(function() { return accountList.itemAtIndex(1) !== null })
            const secondAccount = accountList.itemAtIndex(1)
            secondAccount.clicked()
            tryCompare(fixture.control, "selectedIndex", 1)
            compare(fixture.control.selectedAddress, "b".repeat(64))
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
                model.append({
                    name: "Account " + index,
                    address: String(index).repeat(64),
                    balance: String(index),
                    isPublic: true
                })
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
