import QtQuick
import QtTest
import Logos.Wallet as Wallet
import WalletContractTest

Item {
    id: root
    width: 800
    height: 600

    Component {
        id: backendComponent
        AmmWalletReplica { }
    }

    Component {
        id: controlComponent
        Wallet.WalletControl {
            width: 320
            viewportWidth: 800
        }
    }

    TestCase {
        name: "AmmWalletControl"
        when: windowShown

        function createControl() {
            const backend = createTemporaryObject(backendComponent, root)
            verify(backend)
            const control = createTemporaryObject(controlComponent, root, { wallet: backend })
            verify(control)
            mouseClick(findChild(control, "walletConnectButton"))
            return { backend, control }
        }

        function test_acceptedOpenRemainsBusyUntilCompletion() {
            const fixture = createControl()
            compare(fixture.control.busy, true)
            compare(fixture.control.waitingForWallet, true)
            compare(fixture.control.synchronizing, true)
            compare(findChild(fixture.control, "walletConnectButton").enabled, false)

            fixture.backend.completeOpen(true)
            tryCompare(fixture.control, "connected", true)
            compare(fixture.control.busy, false)
            compare(fixture.control.waitingForWallet, false)
        }

        function test_asyncOpenFailureShowsProviderError() {
            const fixture = createControl()
            fixture.backend.completeOpen(false, "Capability authorization failed")

            const dialog = findChild(fixture.control, "walletMessageDialog")
            tryCompare(dialog, "opened", true)
            verify(dialog.message.indexOf("Capability authorization failed") >= 0)
            compare(fixture.control.connected, false)
            compare(fixture.control.busy, false)
            compare(fixture.control.waitingForWallet, false)
            compare(findChild(fixture.control, "walletConnectButton").enabled, true)
        }
    }
}
