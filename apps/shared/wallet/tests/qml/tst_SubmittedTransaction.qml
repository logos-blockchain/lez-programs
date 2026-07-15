import QtQuick
import QtTest
import Logos.Wallet as Wallet

Item {
    id: root
    width: 360
    height: 400

    Component {
        id: resultComponent

        Wallet.SubmittedTransaction {
            width: 280
        }
    }

    TestCase {
        name: "SubmittedTransaction"
        when: windowShown

        function test_presentsBase58AndCopyFeedback() {
            const result = createTemporaryObject(resultComponent, root, {
                transactionId: "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
            })
            verify(result, "Submitted result exists")
            verify(result.base58Shape)

            const label = findChild(result, "submittedTransactionId")
            verify(label, "Transaction label exists")
            compare(label.text, result.transactionId)
            verify(label.implicitHeight > 0)

            const copyButton = findChild(result, "copySubmittedTransactionButton")
            verify(copyButton, "Copy button exists")
            mouseClick(copyButton)
            verify(copyButton.copied)

            result.transactionId = "0OIl"
            verify(!result.base58Shape)
        }
    }
}
