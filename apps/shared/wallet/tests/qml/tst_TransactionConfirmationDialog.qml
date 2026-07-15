pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtTest
import Logos.Wallet as Wallet

Item {
    id: root
    width: 800
    height: 600

    Component {
        id: summaryComponent

        Item {
            property var snapshot: ({})
            implicitHeight: summaryText.implicitHeight

            Label {
                id: summaryText
                objectName: "customSummaryText"
                text: parent.snapshot.amount || ""
            }
        }
    }

    Component {
        id: dialogComponent

        Wallet.TransactionConfirmationDialog {
            summary: summaryComponent
        }
    }

    Component {
        id: viewportComponent

        Item {
            width: 360
            height: 240
        }
    }

    Component {
        id: tallSummaryComponent

        Item {
            property var snapshot: ({})
            implicitHeight: 360
        }
    }

    Component {
        id: constrainedDialogComponent

        Wallet.TransactionConfirmationDialog {
            summary: tallSummaryComponent
        }
    }

    TestCase {
        name: "TransactionConfirmationDialog"
        when: windowShown

        function test_capturesSnapshotAndLoadsProgramSummary() {
            const dialog = createTemporaryObject(dialogComponent, root)
            verify(dialog, "Dialog exists")
            const source = { amount: "10" }
            dialog.openWithSnapshot(source)
            tryCompare(dialog, "opened", true)
            source.amount = "20"
            compare(dialog.snapshot.amount, "10")

            const summary = findChild(dialog, "customSummaryText")
            verify(summary, "Custom summary exists")
            compare(summary.text, "10")

            confirmedSpy.target = dialog
            confirmedSpy.signalName = "confirmed"
            confirmedSpy.clear()
            mouseClick(findChild(dialog, "transactionConfirmButton"))
            compare(confirmedSpy.count, 1)
            compare(confirmedSpy.signalArguments[0][0].amount, "10")
            tryCompare(dialog, "opened", false)
        }

        function test_busyStateCannotBeDismissed() {
            const dialog = createTemporaryObject(dialogComponent, root)
            verify(dialog, "Dialog exists")
            dialog.openWithSnapshot({ amount: "5" })
            dialog.busy = true
            const cancelButton = findChild(dialog, "transactionCancelButton")
            const confirmButton = findChild(dialog, "transactionConfirmButton")
            verify(!cancelButton.enabled)
            verify(!confirmButton.enabled)
            keyClick(Qt.Key_Escape)
            verify(dialog.opened)
            dialog.cancel()
            verify(dialog.opened)
            dialog.busy = false
            dialog.cancel()
            tryCompare(dialog, "opened", false)
        }

        function test_keepsActionsInsideShortViewport() {
            const viewport = createTemporaryObject(viewportComponent, root)
            verify(viewport, "Short viewport exists")
            const dialog = createTemporaryObject(constrainedDialogComponent, viewport)
            verify(dialog, "Dialog exists")

            dialog.openWithSnapshot({ amount: "5" })
            tryCompare(dialog, "opened", true)
            verify(dialog.height <= viewport.height - 32)
            verify(dialog.y >= 0)
            verify(dialog.y + dialog.height <= viewport.height)

            const confirmButton = findChild(dialog, "transactionConfirmButton")
            verify(confirmButton, "Confirm button exists")
            verify(confirmButton.visible)
        }
    }

    SignalSpy { id: confirmedSpy }
}
