pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/pages" as Pages

Item {
    id: root

    width: 800
    height: 700

    Component {
        id: pageComponent

        Pages.SwapPage {
            width: root.width
            height: root.height
        }
    }

    TestCase {
        name: "SwapPage"
        when: windowShown

        function test_tradeIsExplicitPreviewAndPreservesDraft() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const notice = findChild(page, "swapPreviewNotice")
            const card = findChild(page, "swapCard")
            const dialog = findChild(page, "swapPreviewDialog")
            verify(notice)
            verify(card)
            verify(dialog)
            verify(notice.text.indexOf("Preview only") >= 0)
            verify(notice.text.indexOf("No swap will be submitted") >= 0)

            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.sellInput = "1"
            card.editingSide = "sell"
            tryCompare(card, "canSubmit", true)
            compare(card.submitButtonText, "Preview swap")

            card.previewRequested(card.buildSnapshot())
            tryCompare(dialog, "opened", true)
            compare(dialog.title, "Swap preview")
            compare(findChild(dialog, "transactionConfirmButton").text, "Done")

            dialog.confirm()
            tryCompare(dialog, "opened", false)
            compare(card.sellInput, "1")
        }
    }
}
