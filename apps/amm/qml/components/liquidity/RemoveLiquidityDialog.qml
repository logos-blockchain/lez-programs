import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import "AmountMath.js" as AmountMath

// Remove-liquidity sheet: pick how much of the position to withdraw, preview what
// comes back, submit. Modelled on Uniswap's remove flow — percentage presets over
// a slider, with the two token amounts previewed underneath.
//
// The caller supplies the position (lpBalance + the holdings involved); this
// component owns only the amount selection and the backend round-trips.
Popup {
    id: root

    required property var theme
    // Real backend replica (logos.module("amm_ui")) and the watch runtime.
    property var backend: null
    property var runtime: null

    // Pair display + ids, from the pool detail view.
    property string symbolA: ""
    property string symbolB: ""
    property string tokenAId: ""
    property string tokenBId: ""
    // The balance of the single LP holding being burned from, and the accounts the
    // submit names. A burn names one LP account, so this is the withdrawal's ceiling.
    property string lpBalance: "0"
    // The position's total LP across every holding. Each add mints into a fresh LP
    // account, so a wallet that added twice holds two for one pool and the total can
    // exceed what a single withdrawal reaches.
    property string lpBalanceTotal: "0"
    property string lpHoldingId: ""
    readonly property bool positionIsSplit: AmountMath.isUnsigned(root.lpBalanceTotal)
                                            && AmountMath.compare(root.lpBalanceTotal,
                                                                  root.lpBalance) > 0
    property string holdingAId: ""
    property string holdingBId: ""

    property int slippageBps: 50

    // 1..100. Percent rather than a raw amount: it is what the presets and the
    // slider both drive, and it keeps "all of it" exact (see lpAmountRaw).
    property int percent: 50

    // ── Quote state (backend.removeLiquidityQuote) ───────────────────────────
    property bool quoteLoading: false
    property string quoteError: ""
    property string amountARaw: "0"
    property string amountBRaw: "0"
    property string minimumAmountARaw: "0"
    property string minimumAmountBRaw: "0"
    property bool quoteReady: false

    property bool submitting: false
    property string submitError: ""

    // Monotonic tag: the slider fires quotes faster than they return, and an
    // earlier reply must not overwrite a later percentage's preview.
    property int quoteGeneration: 0

    signal removed(string transactionId)

    // 100% burns the whole balance exactly; anything else floors, so the dust
    // stays in the position rather than rounding the request above the balance.
    readonly property string lpAmountRaw: root.percent >= 100
        ? AmountMath.normalize(root.lpBalance)
        : AmountMath.mulDivFloor(root.lpBalance, String(root.percent), "100")

    readonly property bool hasAmount: AmountMath.isUnsigned(root.lpAmountRaw)
                                      && AmountMath.normalize(root.lpAmountRaw) !== "0"
    readonly property bool canSubmit: root.hasAmount
                                      && root.quoteReady
                                      && !root.quoteLoading
                                      && !root.submitting
                                      && root.quoteError.length === 0
                                      && root.lpHoldingId.length > 0
                                      && root.holdingAId.length > 0
                                      && root.holdingBId.length > 0

    // Opens the sheet on a fresh position, resetting the previous visit's state.
    function openFor(position) {
        root.symbolA = String(position.symbolA || "")
        root.symbolB = String(position.symbolB || "")
        root.tokenAId = String(position.tokenAId || "")
        root.tokenBId = String(position.tokenBId || "")
        root.lpBalance = String(position.lpBalance || "0")
        root.lpBalanceTotal = String(position.lpBalanceTotal || position.lpBalance || "0")
        root.lpHoldingId = String(position.lpHoldingId || "")
        root.holdingAId = String(position.holdingAId || "")
        root.holdingBId = String(position.holdingBId || "")
        root.percent = 50
        root.quoteGeneration++
        quoteDebounce.stop()
        root.quoteLoading = false
        root.submitting = false
        root.quoteError = ""
        root.submitError = ""
        root.quoteReady = false
        root.amountARaw = "0"
        root.amountBRaw = "0"
        root.minimumAmountARaw = "0"
        root.minimumAmountBRaw = "0"
        root.open()
        root.requestQuote()
    }

    onPercentChanged: root.requestQuote()
    onSlippageBpsChanged: root.requestQuote()

    Timer {
        id: quoteDebounce

        interval: 250
        repeat: false
        onTriggered: root.doQuote()
    }

    function requestQuote() {
        root.quoteReady = false
        if (!root.opened)
            return
        quoteDebounce.restart()
    }

    function doQuote() {
        if (!root.backend || !root.runtime || !root.hasAmount)
            return

        const generation = ++root.quoteGeneration
        root.quoteLoading = true
        root.runtime.watch(root.backend.removeLiquidityQuote({
            "tokenAId": root.tokenAId,
            "tokenBId": root.tokenBId,
            "lpAmountRaw": root.lpAmountRaw,
            "slippageBps": root.slippageBps
        }),
            function(quote) {
                if (generation !== root.quoteGeneration)
                    return
                root.quoteLoading = false
                if (quote && quote.status === "ok") {
                    root.amountARaw = String(quote.amountARaw || "0")
                    root.amountBRaw = String(quote.amountBRaw || "0")
                    root.minimumAmountARaw = String(quote.minimumAmountARaw || "0")
                    root.minimumAmountBRaw = String(quote.minimumAmountBRaw || "0")
                    root.quoteError = ""
                    root.quoteReady = true
                    return
                }
                root.quoteReady = false
                root.quoteError = root.issueText((quote && quote.error) || "backend_error")
            },
            function(error) {
                if (generation !== root.quoteGeneration)
                    return
                console.warn("removeLiquidityQuote error:", error)
                root.quoteLoading = false
                root.quoteReady = false
                root.quoteError = root.issueText("backend_error")
            })
    }

    function submit() {
        if (!root.canSubmit || !root.backend || !root.runtime)
            return

        root.submitting = true
        root.submitError = ""
        root.runtime.watch(root.backend.removeLiquidity({
            "tokenAId": root.tokenAId,
            "tokenBId": root.tokenBId,
            "holdingAId": root.holdingAId,
            "holdingBId": root.holdingBId,
            "lpHoldingId": root.lpHoldingId,
            "lpAmountRaw": root.lpAmountRaw,
            // The floors the quote computed for this exact amount, so the submit
            // enforces the slippage the preview promised.
            "minAmountARaw": root.minimumAmountARaw,
            "minAmountBRaw": root.minimumAmountBRaw,
            // u64-max sentinel = no deadline, same as the other submits.
            "deadlineMs": "18446744073709551615"
        }),
            function(result) {
                root.submitting = false
                if (result && result.status === "ok"
                        && String(result.transactionId || "").length > 0) {
                    root.removed(String(result.transactionId))
                    root.close()
                    return
                }
                root.submitError = root.issueText((result && result.error)
                                                  || "wallet_submission_failed")
            },
            function(error) {
                console.warn("removeLiquidity error:", error)
                root.submitting = false
                root.submitError = root.issueText("wallet_submission_failed")
            })
    }

    function issueText(code) {
        switch (String(code)) {
        case "no_pool":
            return qsTr("This pool no longer exists on-chain.")
        case "insufficient_pool_liquidity":
            return qsTr("The pool cannot release this much — some liquidity is permanently locked.")
        case "amount_too_low":
            return qsTr("This amount is too small to withdraw anything.")
        case "minimum_amount_zero":
            return qsTr("This amount rounds to nothing on one side. Withdraw more.")
        case "invalid_slippage":
            return qsTr("The slippage tolerance is out of range.")
        case "pair_mismatch":
            return qsTr("The pool does not match this token pair.")
        case "wallet_unavailable":
            return qsTr("Connect a wallet to withdraw.")
        case "wallet_submission_failed":
            return qsTr("The withdrawal could not be submitted.")
        case "config_missing":
            return qsTr("The AMM config account has not been initialized.")
        default:
            return qsTr("Withdrawal failed: %1").arg(String(code))
        }
    }

    // Group an exact decimal string; the amounts are u128 base units.
    function amountText(rawValue) {
        var digits = String(rawValue).replace(/[^0-9]/g, "").replace(/^0+(?=[0-9])/, "")
        if (digits.length === 0)
            return "0"
        var separator = Qt.locale().groupSeparator
        var grouped = ""
        for (var i = 0; i < digits.length; ++i) {
            if (i > 0 && (digits.length - i) % 3 === 0)
                grouped += separator
            grouped += digits[i]
        }
        return grouped
    }

    objectName: "removeLiquidityDialog"
    modal: true
    anchors.centerIn: Overlay.overlay
    width: Math.min(420, (parent ? parent.width : 420) - 32)
    padding: 20
    closePolicy: root.submitting ? Popup.NoAutoClose
                                 : (Popup.CloseOnEscape | Popup.CloseOnPressOutside)

    background: Rectangle {
        radius: 20
        color: root.theme.colors.cardBg
        border.color: root.theme.colors.border
        border.width: 1
    }

    Overlay.modal: Rectangle {
        color: "#B0000000"
    }

    contentItem: ColumnLayout {
        spacing: 18

        Text {
            Layout.fillWidth: true
            text: qsTr("Remove liquidity")
            color: root.theme.colors.textPrimary
            font.pixelSize: 20
            font.weight: Font.Bold
            elide: Text.ElideRight
        }

        // ── Amount picker ────────────────────────────────────────────────────
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12

            Text {
                objectName: "removePercentLabel"
                Layout.fillWidth: true
                text: qsTr("%1%").arg(root.percent)
                color: root.theme.colors.textPrimary
                font.pixelSize: 34
                font.weight: Font.Bold
                horizontalAlignment: Text.AlignHCenter
            }

            Slider {
                id: percentSlider

                objectName: "removePercentSlider"
                Layout.fillWidth: true
                from: 1
                to: 100
                stepSize: 1
                value: root.percent
                onMoved: root.percent = Math.round(value)
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Repeater {
                    model: [25, 50, 75, 100]

                    delegate: Rectangle {
                        id: preset

                        required property int modelData

                        readonly property bool current: root.percent === preset.modelData

                        objectName: "removePreset%1".arg(preset.modelData)
                        Layout.fillWidth: true
                        Layout.preferredHeight: 34
                        radius: 8
                        color: preset.current ? root.theme.colors.selection
                                              : root.theme.colors.inputBg
                        border.color: preset.current ? root.theme.colors.ctaBg
                                                     : root.theme.colors.borderStrong
                        border.width: 1

                        Accessible.role: Accessible.Button
                        Accessible.name: presetLabel.text

                        Text {
                            id: presetLabel

                            anchors.centerIn: parent
                            text: preset.modelData === 100
                                  ? qsTr("Max") : qsTr("%1%").arg(preset.modelData)
                            color: root.theme.colors.textPrimary
                            font.pixelSize: 13
                            font.weight: preset.current ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.percent = preset.modelData
                        }
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: root.theme.colors.divider
        }

        // ── Preview ──────────────────────────────────────────────────────────
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: qsTr("You receive")
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                font.weight: Font.DemiBold
            }

            AmountLine {
                objectName: "removeReceiveA"
                symbol: root.symbolA
                amount: root.quoteReady ? root.amountText(root.amountARaw) : qsTr("—")
            }

            AmountLine {
                objectName: "removeReceiveB"
                symbol: root.symbolB
                amount: root.quoteReady ? root.amountText(root.amountBRaw) : qsTr("—")
            }

            Text {
                Layout.fillWidth: true
                visible: root.quoteReady
                text: qsTr("At least %1 %2 and %3 %4 after slippage.")
                      .arg(root.amountText(root.minimumAmountARaw)).arg(root.symbolA)
                      .arg(root.amountText(root.minimumAmountBRaw)).arg(root.symbolB)
                color: root.theme.colors.textPlaceholder
                font.pixelSize: 11
                wrapMode: Text.Wrap
            }

            Text {
                objectName: "removeSplitPositionNote"
                Layout.fillWidth: true
                visible: root.positionIsSplit
                text: qsTr("A withdrawal burns one LP account at a time. This one holds %1 of your %2 LP; repeat to withdraw the rest.")
                      .arg(root.amountText(root.lpBalance))
                      .arg(root.amountText(root.lpBalanceTotal))
                color: root.theme.colors.textPlaceholder
                font.pixelSize: 11
                wrapMode: Text.Wrap
            }

            Text {
                objectName: "removeDialogStatus"
                Layout.fillWidth: true
                visible: text.length > 0
                text: root.submitError.length > 0 ? root.submitError
                      : root.quoteError.length > 0 ? root.quoteError
                      : root.quoteLoading ? qsTr("Loading preview…") : ""
                color: root.submitError.length > 0 || root.quoteError.length > 0
                       ? root.theme.colors.error : root.theme.colors.textSecondary
                font.pixelSize: 12
                wrapMode: Text.Wrap
            }
        }

        // ── Actions ──────────────────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            AmmSecondaryButton {
                objectName: "removeCancelButton"
                theme: root.theme
                text: qsTr("Cancel")
                enabled: !root.submitting
                Layout.fillWidth: true
                onClicked: root.close()
            }

            AmmPrimaryButton {
                objectName: "removeConfirmButton"
                theme: root.theme
                text: root.submitting ? qsTr("Removing…") : qsTr("Remove")
                enabled: root.canSubmit
                implicitHeight: 44
                Layout.fillWidth: true
                onClicked: root.submit()
            }
        }
    }

    component AmountLine: RowLayout {
        id: line

        property string symbol: ""
        property string amount: ""

        Layout.fillWidth: true
        spacing: 10

        Text {
            text: line.symbol
            color: root.theme.colors.textSecondary
            font.pixelSize: 14
            elide: Text.ElideRight
            Layout.fillWidth: true
            Layout.preferredWidth: implicitWidth
        }

        Text {
            text: line.amount
            color: root.theme.colors.textPrimary
            font.pixelSize: 14
            font.weight: Font.Medium
            horizontalAlignment: Text.AlignRight
            elide: Text.ElideRight
            Layout.fillWidth: true
            Layout.preferredWidth: implicitWidth
        }
    }
}
