pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/components/liquidity" as Liquidity

TestCase {
    id: testCase

    name: "LiquidityConfirmationSummary"

    Component {
        id: summaryComponent

        Liquidity.LiquidityConfirmationSummary {
            width: 480
        }
    }

    Component {
        id: clipboardSinkComponent

        TextEdit {}
    }

    SignalSpy {
        id: editedSpy
        signalName: "snapshotEdited"
    }

    function test_protocolActionsUseDisplayLabels() {
        var summary = createTemporaryObject(summaryComponent, testCase)
        verify(summary)

        compare(summary.actionText("NewDefinition"), "Create pool")
        compare(summary.actionText("AddLiquidity"), "Add liquidity")
        compare(summary.actionText(""), "-")
    }

    function test_lpDestinationOffersExistingHoldingsAndCreateNew() {
        var summary = createTemporaryObject(summaryComponent, testCase, {
            "snapshot": {
                "instruction": "AddLiquidity",
                "request": ({ "schema": "new-position.v2" }),
                "lpHoldingOptions": [{
                    "holdingId": "44444444444444444444444444444444",
                    "balanceRaw": "7"
                }],
                "lpDestinationRequired": true,
                "quoteReady": false
            }
        })
        verify(summary)
        editedSpy.target = summary
        editedSpy.clear()

        var rows = summary.destinationRows()
        compare(rows.length, 2)
        compare(rows[1].createFresh, true)
        summary.selectDestination(rows[0])
        compare(editedSpy.count, 1)
        compare(editedSpy.signalArguments[0][0].request.lpHoldingId,
                "44444444444444444444444444444444")
        compare(editedSpy.signalArguments[0][0].quoteReady, false)
        editedSpy.target = null
    }

    function test_lpDestinationPickerUsesAmmTheme() {
        var summary = createTemporaryObject(summaryComponent, testCase, {
            "snapshot": {
                "instruction": "AddLiquidity",
                "request": ({ "schema": "new-position.v2" }),
                "lpHoldingOptions": [{
                    "holdingId": "44444444444444444444444444444444",
                    "balanceRaw": "7"
                }],
                "lpDestinationRequired": true,
                "quoteReady": false
            }
        })
        verify(summary)

        var picker = findChild(summary, "lpDestinationPicker")
        verify(picker)
        compare(picker.background.color, summary.theme.colors.panelBg)
    }

    function test_confirmationShowsQuoteDetails() {
        var summary = createTemporaryObject(summaryComponent, testCase, {
            "snapshot": {
                "instruction": "NewDefinition",
                "poolStatus": "missing_pool",
                "depositLabel": "Opening deposit",
                "depositAText": "2 Low",
                "depositBText": "3 High",
                "initialPriceText": "1 Low = 1.5 High",
                "inverseInitialPriceText": "1 High = 0.66 Low",
                "depositMultiplierText": "2x minimum",
                "depositScaleText": "20000 basis points",
                "expectedLpText": "10 raw LP",
                "lpGuardLabel": "Locked LP",
                "lpGuardText": "2 raw LP",
                "poolId": "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE",
                "accountPreview": [{
                    "order": 0,
                    "role": "LP holding",
                    "action": "create",
                    "accountId": "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"
                }]
            }
        })
        verify(summary)

        var deposit = findChild(summary, "confirmationDeposit")
        var initialPrice = findChild(summary, "confirmationInitialPrice")
        var inversePrice = findChild(summary, "confirmationInversePrice")
        var multiplier = findChild(summary, "confirmationDepositMultiplier")
        var scale = findChild(summary, "confirmationDepositScale")
        var expectedLp = findChild(summary, "confirmationExpectedLp")
        var lpGuard = findChild(summary, "confirmationLpGuard")
        var pool = findChild(summary, "poolAddressRow")
        var accountPlan = findChild(summary, "confirmationAccountPlan")

        verify(deposit)
        verify(initialPrice)
        verify(inversePrice)
        verify(multiplier)
        verify(scale)
        verify(expectedLp)
        verify(lpGuard)
        verify(pool)
        verify(accountPlan)
        compare(deposit.label, "Opening deposit")
        compare(deposit.value, "2 Low + 3 High")
        compare(initialPrice.value, "1 Low = 1.5 High")
        compare(inversePrice.value, "1 High = 0.66 Low")
        compare(multiplier.value, "2x minimum")
        compare(scale.value, "20000 basis points")
        compare(expectedLp.value, "10 raw LP")
        compare(lpGuard.label, "Locked LP")
        compare(lpGuard.value, "2 raw LP")
        compare(pool.address, "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE")
        compare(accountPlan.text, "Account plan (1)")
    }

    function test_addressesStayOnOneLineAndCopyFullBase58Value() {
        failOnWarning(/Detected recursive rearrange/)

        var address = "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"
        var summary = createTemporaryObject(summaryComponent, testCase, {
            "width": 250,
            "snapshot": {
                "poolId": address,
                "accountPreview": [{
                    "order": 0,
                    "role": "LP holding",
                    "action": "create",
                    "accountId": address
                }]
            }
        })
        var sink = createTemporaryObject(clipboardSinkComponent, testCase)
        verify(summary)
        verify(sink)
        wait(0)

        var poolRow = findChild(summary, "poolAddressRow")
        verify(poolRow)
        var addressText = findChild(poolRow, "addressText")
        var copyButton = findChild(poolRow, "copyAddressButton")
        verify(addressText)
        verify(copyButton)
        compare(addressText.text, address)
        compare(addressText.wrapMode, Text.NoWrap)
        compare(addressText.elide, Text.ElideMiddle)
        tryCompare(addressText, "truncated", true)

        copyButton.clicked()
        sink.paste()
        tryCompare(sink, "text", address)

        var accountPlan = findChild(summary, "confirmationAccountPlan")
        verify(accountPlan)
        accountPlan.clicked()
        wait(0)
        var accountRow = findChild(summary, "accountPlanAddressRow0")
        verify(accountRow)
        compare(accountRow.address, address)

        sink.text = ""
        var accountCopyButton = findChild(accountRow, "copyAddressButton")
        verify(accountCopyButton)
        accountCopyButton.clicked()
        sink.paste()
        tryCompare(sink, "text", address)
    }
}
