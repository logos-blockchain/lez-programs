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

        function test_themeToggleReceivesPointerInput() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const toggle = findChild(page, "swapThemeToggle")
            verify(toggle)
            compare(page.darkTheme, true)

            const position = toggle.mapToItem(page, toggle.width / 2, toggle.height / 2)
            mouseClick(page, position.x, position.y)

            tryCompare(page, "darkTheme", false)
        }

        function test_exactOutputUsesMaximumInputLimit() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const card = findChild(page, "swapCard")
            const dialog = findChild(page, "swapPreviewDialog")
            verify(card)
            verify(dialog)

            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.slippageTolerancePercent = 10
            card.buyInput = "1000"
            card.editingSide = "buy"
            tryCompare(card, "canSubmit", true)

            const snapshot = card.buildSnapshot()
            compare(snapshot.swapMode, "swap-exact-output")
            compare(snapshot.buyAmount, "1000.00")
            compare(snapshot.minReceived, "")
            verify(snapshot.maxSent.length > 0)
            verify(Number(snapshot.maxSent) > Number(snapshot.sellAmount))

            dialog.openWithSnapshot(snapshot)
            tryCompare(dialog, "opened", true)
            tryCompare(findChild(dialog, "swapPayLabel"), "text", "You pay at most")
            compare(findChild(dialog, "swapPayAmount").text, snapshot.maxSent + " " + snapshot.sellToken)
            compare(findChild(dialog, "swapReceiveLabel").text, "You receive")
            compare(findChild(dialog, "swapReceiveAmount").text, snapshot.buyAmount + " " + snapshot.buyToken)
            compare(findChild(dialog, "swapLimitLabel").text, "Max sent")
            compare(findChild(dialog, "swapLimitValue").text, snapshot.maxSent + " " + snapshot.sellToken)
        }

        function test_exactInputUsesMinimumOutputLimit() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const card = findChild(page, "swapCard")
            verify(card)
            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.slippageTolerancePercent = 10
            card.sellInput = "1"
            card.editingSide = "sell"
            tryCompare(card, "canSubmit", true)

            const snapshot = card.buildSnapshot()
            compare(snapshot.swapMode, "swap-exact-input")
            compare(snapshot.sellAmount, "1.00")
            compare(snapshot.maxSent, "")
            verify(snapshot.minReceived.length > 0)
            verify(Number(snapshot.minReceived) < Number(snapshot.buyAmount))
        }

        function test_exactOutputRequiresBalanceForMaximumInput() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const card = findChild(page, "swapCard")
            verify(card)
            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.slippageTolerancePercent = 10
            card.buyInput = "11500"
            card.editingSide = "buy"

            verify(card.parsedSellAmount < card.sellToken.balance)
            verify(card.maxSentAmount > card.sellToken.balance)
            verify(card.insufficientBalance)
            verify(!card.canSubmit)
            compare(card.submitButtonText, "Insufficient balance")
        }

        function test_exactOutputCannotConsumeEntireReserve() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const card = findChild(page, "swapCard")
            verify(card)
            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.buyInput = String(card.buyToken.reserve)
            card.editingSide = "buy"

            compare(card.parsedBuyAmount, card.buyToken.reserve)
            verify(card.insufficientLiquidity)
            verify(!card.canSubmit)
            compare(card.submitButtonText, "Insufficient liquidity")
        }

        function test_reselectingOppositeTokenSwapsPair() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const card = findChild(page, "swapCard")
            verify(card)
            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.setToken("buy", page.tokens[0])

            compare(card.sellToken.address, page.tokens[1].address)
            compare(card.buyToken.address, page.tokens[0].address)
            verify(!card.sameTokenSelected)
        }

        function test_sameTokenPairCannotBePreviewed() {
            const page = createTemporaryObject(pageComponent, root)
            verify(page)

            const card = findChild(page, "swapCard")
            verify(card)
            card.sellToken = page.tokens[0]
            card.buyToken = page.tokens[0]
            card.sellInput = "1"
            card.editingSide = "sell"

            verify(card.sameTokenSelected)
            verify(!card.canSubmit)
            compare(card.submitButtonText, "Select different tokens")
        }

        function test_tradeContentStaysReachable_data() {
            return [
                { "tag": "short", "width": 360, "height": 184 },
                { "tag": "expanded", "width": 400, "height": 544 }
            ]
        }

        function test_tradeContentStaysReachable(data) {
            const page = createTemporaryObject(pageComponent, root, {
                "width": data.width,
                "height": data.height
            })
            verify(page)

            const scroll = findChild(page, "swapScroll")
            const card = findChild(page, "swapCard")
            const notice = findChild(page, "swapPreviewNotice")
            verify(scroll)
            verify(card)
            verify(notice)
            card.setToken("sell", page.tokens[0])
            card.setToken("buy", page.tokens[1])
            card.sellInput = "1"
            card.editingSide = "sell"
            wait(0)

            const cardPosition = card.mapToItem(page, 0, 0)
            verify(cardPosition.x >= 16)
            verify(cardPosition.x + card.width <= page.width - 16)

            if (scroll.contentHeight > scroll.height)
                scroll.contentY = scroll.contentHeight - scroll.height
            wait(0)
            const noticePosition = notice.mapToItem(scroll, 0, 0)
            verify(noticePosition.y >= 0)
            verify(noticePosition.y + notice.height <= scroll.height)
        }
    }
}
