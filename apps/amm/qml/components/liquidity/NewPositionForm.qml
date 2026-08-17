pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Controls
import Logos.Icons
import Logos.Wallet

import "AmountMath.js" as AmountMath

AmmActionCard {
    id: root

    theme: fallbackTheme

    AmmTheme {
        id: fallbackTheme
    }

    property var flowState: ({})
    // True while the app is (re)loading the token selector rows (backend.resolveTokens());
    // gates the selectors/spinner like the old context load did.
    property bool loadingTokens: false
    // Wallet token holdings (backend.tokenHoldings()) for the create-pool account
    // selectors, narrowed per side by the selected token's base58 definitionId. The
    // chosen holdings feed the createPool call via submissionSnapshot().
    property var holdings: []
    readonly property string selectedHoldingAId: tokenAInput.selectedHoldingId
    readonly property string selectedHoldingBId: tokenBInput.selectedHoldingId
    readonly property string selectedBalanceA: tokenAInput.selectedBalance
    readonly property string selectedBalanceB: tokenBInput.selectedBalance
    property string selectedTokenAId: ""
    property string selectedTokenBId: ""
    property int selectedFeeBps: 30
    property int slippageBps: 50
    property string amountA: ""
    property string amountB: ""
    property string priceAmountA: "1"
    property string priceAmountB: "1"
    property string minimumAmountA: ""
    property string minimumAmountB: ""
    property var localErrors: []
    property string resolvingTokenId: ""
    property string resolvingTokenSide: ""
    property string tokenResolutionError: ""
    property string tokenResolutionErrorSide: ""
    property string tokenResolutionMessage: ""
    property var activePoolQuote: ({})
    property string headingText: qsTr("New position")
    property string headingDetail: ""
    property bool showRefreshAction: true

    readonly property var quotePayload: root.flowState.quote || ({})
    readonly property bool contextLoading: root.loadingTokens
    readonly property bool quoteLoading: root.flowState.quoteLoading === true
    readonly property bool submitting: root.flowState.submitting === true
    readonly property bool quoteStale: root.flowState.quoteStale === true
    readonly property string submitError: root.flowState.errorCode
                                                  ? root.issueText(root.flowState.errorCode) : ""
    readonly property string transactionId: String(root.flowState.transactionId || "")
    readonly property var emptyToken: ({
        "definitionId": "",
        "name": "",
        "totalSupply": "0",
        "balance": "0"
    })
    // The liquidity token selector rows, injected from the app (backend.resolveTokens()):
    // the union of configured tokens and persisted-custom tokens. Every row is
    // { definitionId (base58), name, totalSupply, holdingId, balance } and already valid
    // (unresolvable ids are omitted upstream), so every listed token is selectable.
    property var tokens: []
    // The ids currently offered by the selector — a readable projection of `tokens` (every
    // listed token is selectable). Exposed as a property so it can be observed directly.
    readonly property var selectableTokenIdList: root.selectableTokenIds()
    // Same set as a comma-joined string — a form that serializes reliably over the QML
    // inspector (var arrays may not), used by the custom-token test.
    readonly property string selectableTokenIdsCsv: root.selectableTokenIds().join(",")
    // Whether the wallet session is ready (from the flow); gates funding/selection like the
    // old context "ready"/"no_wallet" status did, minus the network envelope.
    property bool walletReady: false
    // Supported fee tiers as raw bps, injected from backend.feeTiers() (amm_core's
    // SUPPORTED_FEE_TIERS). The selector's delegate wants { feeBps } rows, so wrap
    // each int; labels are derived locally via feeLabel().
    property var feeTiers: []
    readonly property var feeTierModel: (root.feeTiers || []).map(function(bps) {
        return { "feeBps": Number(bps) }
    })
    readonly property var tokenA: root.tokenById(root.selectedTokenAId)
    readonly property var tokenB: root.tokenById(root.selectedTokenBId)
    readonly property int decimalsA: 0
    readonly property int decimalsB: 0
    readonly property bool displayIsCanonical: root.selectedTokenAId.length > 0
                                               && AmountMath.compareBase58Ids(root.selectedTokenAId,
                                                                              root.selectedTokenBId) > 0
    readonly property int canonicalDecimalsA: root.displayIsCanonical ? root.decimalsA : root.decimalsB
    readonly property int canonicalDecimalsB: root.displayIsCanonical ? root.decimalsB : root.decimalsA
    readonly property string initialPrice: AmountMath.ratioValue(root.priceAmountA,
                                                                  root.priceAmountB,
                                                                  12)
    readonly property string inverseInitialPrice: AmountMath.ratioValue(root.priceAmountB,
                                                                         root.priceAmountA,
                                                                         12)
    // Create-vs-add comes from the flow's resolvePool read (pool existence), not the quote.
    // undefined ⇒ not resolved yet (neither branch shown).
    readonly property bool activePool: root.flowState.poolExists === true
    readonly property bool missingPool: root.flowState.poolExists === false
    readonly property int poolFeeBps: root.knownPoolFeeBps()
    readonly property bool compact: root.width < 420
    readonly property bool hasPair: root.selectedTokenAId.length > 0
                                    && root.selectedTokenBId.length > 0
                                    && root.selectedTokenAId !== root.selectedTokenBId
    readonly property bool resolvingToken: root.resolvingTokenId.length > 0
    // Both createPool and addLiquidity submit caller-provided A/B holdings (see
    // submissionSnapshot), so once a pool status is known both require the two selectors
    // resolved (the selectors are shown for both branches).
    readonly property bool holdingsReady: !(root.missingPool || root.activePool)
                                          || (root.selectedHoldingAId.length > 0
                                              && root.selectedHoldingBId.length > 0)
    // Both deposit amounts must be present. For create the quote auto-fills them from the
    // opening deposit; for add the user enters one and the other ratio-fills. Without this
    // an active-pool probe quote (sent on pair-select with simulated amounts) would enable
    // the CTA before any amount is entered.
    readonly property bool hasDepositAmounts: root.amountA.length > 0 && root.amountB.length > 0
    // A successful lean quote is submittable — funding is gated below (holdings + amounts),
    // not by a quote-side canSubmit flag.
    readonly property bool canConfirm: root.quotePayload.status === "ok"
                                       && root.quoteMatchesPair()
                                       && root.holdingsReady
                                       && root.hasDepositAmounts
                                       // The lean quotes don't check funding, so gate the CTA on
                                       // the entered amounts fitting the selected holdings' balances.
                                       && root.fundingSufficient
                                       && !root.contextLoading
                                       && !root.quoteLoading
                                       && !root.quoteStale
                                       && !root.submitting
                                       // A create-pool tx for this pair is already in flight
                                       // (transactionId set) but the pool still reads missing until
                                       // the chain processes it — block confirm so a stale
                                       // missing_pool quote can't submit a duplicate NewDefinition.
                                       && !(root.missingPool && root.transactionId.length > 0)
    // Per-side funding check, decoupled from buildQuoteRequest/the quote: the deposit each side
    // spends must fit its selected holding's balance (the lean createPoolQuote / addLiquidityQuote
    // ops never compare amount to balance, so a submit would otherwise fail on an
    // insufficient-balance transfer). amountA / selectedBalanceA are both the display token-A
    // side, so no canonical reorientation is needed.
    readonly property bool fundingSufficient: root.fundingError("A").length === 0
                                              && root.fundingError("B").length === 0

    signal quoteRequested(bool immediate, var quoteRequest)
    signal confirmationRequested(var snapshot)
    signal tokenResolveRequested(string tokenId)
    signal draftChanged
    signal pairReset
    signal refreshRequested

    readonly property int contentPadding: width >= 600 ? 24 : 16

    implicitHeight: content.implicitHeight + root.contentPadding * 2
    implicitWidth: 480

    Component.onCompleted: Qt.callLater(root.reconcileSelection)
    // Re-reconcile the current selection whenever the app's token list changes (e.g. after a
    // wallet toggle or a custom token is added). Resolution completion is driven externally
    // (LiquidityPage calls finishTokenResolution once addCustomToken returns).
    onTokensChanged: Qt.callLater(root.reconcileSelection)
    onQuotePayloadChanged: {
        if (root.quoteStale)
            return
        root.rememberActivePoolQuote()
        Qt.callLater(root.applyQuoteSideEffects)
    }

    Component {
        id: priceAmountAAdjustment

        PriceRatioAdjustment {
            amount: root.priceAmountA
            enabled: !root.submitting
            fieldName: "priceAmountAField"
            invalid: root.fieldHasError("initialPrice")
            onEdited: function(value) { root.editPrice("A", value) }
        }
    }

    Component {
        id: priceAmountBAdjustment

        PriceRatioAdjustment {
            amount: root.priceAmountB
            enabled: !root.submitting
            fieldName: "priceAmountBField"
            invalid: root.fieldHasError("initialPrice")
            onEdited: function(value) { root.editPrice("B", value) }
        }
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: root.contentPadding
        spacing: 16

        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 3

                Text {
                    text: root.headingText
                    color: root.theme.colors.textPrimary
                    font.pixelSize: 18
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0
                }

                Text {
                    text: root.headingDetail.length > 0
                          ? root.headingDetail : root.contextStatusText()
                    color: root.theme.colors.textSecondary
                    font.pixelSize: 12
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }

            BusyIndicator {
                running: root.contextLoading || root.quoteLoading
                visible: running
                implicitWidth: 24
                implicitHeight: 24
            }

            LogosIconButton {
                iconSource: LogosIcons.refresh
                iconColor: root.theme.colors.textSecondary
                iconSize: 18
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                enabled: !root.contextLoading && !root.submitting
                visible: root.showRefreshAction
                Accessible.name: qsTr("Refresh position data")
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                onClicked: root.refreshRequested()
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            TokenAmountInput {
                id: tokenAInput

                objectName: "tokenAAmountInput"
                Layout.fillWidth: true
                theme: root.theme
                text: root.amountA
                label: qsTr("Token A amount")
                balance: root.contextLoading ? "" : root.balanceText(root.tokenA, root.decimalsA)
                helperText: root.missingPool && !root.compact
                            ? root.minimumAmountText("A") : ""
                errorText: root.formErrorText()
                invalid: root.fieldHasError("amountA")
                readOnly: root.submitting || (!root.activePool && !root.missingPool)
                showMaxButton: root.activePool
                tokenData: root.tokenA.definitionId ? root.tokenA : null
                tokens: root.tokens
                selectedTokenId: root.selectedTokenAId
                holdings: root.holdings
                holdingDefinitionId: root.selectedTokenAId
                showHoldingSelector: (root.missingPool || root.activePool) && root.hasPair
                selectorObjectName: "newPositionAccountSelectorA"
                tokenInvalid: root.tokenHasError("A")
                tokenSelectionEnabled: !root.contextLoading && !root.submitting
                adjustment: root.missingPool ? priceAmountAAdjustment : null
                adjustmentWidth: root.missingPool ? (root.compact ? 100 : 142) : 0
                adjustmentHeight: root.missingPool ? 24 : 0
                disabledReasonForCode: function(code) { return root.issueText(code) }
                detailForToken: function(token) { return root.tokenBalanceDetail(token) }
                onEditingChanged: function(value) {
                    if (root.activePool)
                        root.editActiveAmount("A", value)
                    else
                        root.editMissingAmount("A", value)
                }
                onEditingCommitted: function(value) {
                    if (root.activePool)
                        root.finishActiveAmount("A", value)
                    else
                        root.finishMissingAmount("A", value)
                }
                onMaxClicked: root.useMaximum()
                onTokenSelected: function(tokenId) { root.resolveToken("A", tokenId) }
                onTokenEntered: function(value) { root.resolveToken("A", value) }
            }

            AmmPairSeparator {
                Layout.fillWidth: true
                theme: root.theme
                enabled: root.hasPair && !root.submitting
                onClicked: root.swapTokens()
            }

            TokenAmountInput {
                id: tokenBInput

                objectName: "tokenBAmountInput"
                Layout.fillWidth: true
                theme: root.theme
                text: root.amountB
                label: qsTr("Token B amount")
                balance: root.contextLoading ? "" : root.balanceText(root.tokenB, root.decimalsB)
                helperText: root.missingPool && !root.compact
                            ? root.minimumAmountText("B") : ""
                invalid: root.fieldHasError("amountB")
                readOnly: root.submitting || (!root.activePool && !root.missingPool)
                showMaxButton: root.activePool
                tokenData: root.tokenB.definitionId ? root.tokenB : null
                tokens: root.tokens
                selectedTokenId: root.selectedTokenBId
                holdings: root.holdings
                holdingDefinitionId: root.selectedTokenBId
                showHoldingSelector: (root.missingPool || root.activePool) && root.hasPair
                selectorObjectName: "newPositionAccountSelectorB"
                tokenInvalid: root.tokenHasError("B")
                tokenSelectionEnabled: !root.contextLoading && !root.submitting
                adjustment: root.missingPool ? priceAmountBAdjustment : null
                adjustmentWidth: root.missingPool ? (root.compact ? 100 : 142) : 0
                adjustmentHeight: root.missingPool ? 24 : 0
                disabledReasonForCode: function(code) { return root.issueText(code) }
                detailForToken: function(token) { return root.tokenBalanceDetail(token) }
                onEditingChanged: function(value) {
                    if (root.activePool)
                        root.editActiveAmount("B", value)
                    else
                        root.editMissingAmount("B", value)
                }
                onEditingCommitted: function(value) {
                    if (root.activePool)
                        root.finishActiveAmount("B", value)
                    else
                        root.finishMissingAmount("B", value)
                }
                onMaxClicked: root.useMaximum()
                onTokenSelected: function(tokenId) { root.resolveToken("B", tokenId) }
                onTokenEntered: function(value) { root.resolveToken("B", value) }
            }
        }

        Text {
            Layout.fillWidth: true
            visible: root.resolvingToken || root.tokenResolutionMessage.length > 0
            text: root.resolvingToken
                  ? qsTr("Resolving TokenDefinition...")
                  : root.tokenResolutionMessage
            color: root.theme.colors.textSecondary
            font.pixelSize: 11
            wrapMode: Text.Wrap
        }

        Text {
            Layout.fillWidth: true
            visible: text.length > 0
            text: root.activePool ? root.activePriceText()
                                  : root.missingPool ? root.depositMultiplierText() : ""
            color: root.theme.colors.textSecondary
            font.pixelSize: 12
            horizontalAlignment: Text.AlignRight
            wrapMode: Text.Wrap
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: root.theme.colors.divider
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: !root.contextLoading

            Text {
                text: qsTr("Fee tier")
                color: root.theme.colors.textPrimary
                font.pixelSize: 13
                font.weight: Font.Medium
            }

            GridLayout {
                Layout.fillWidth: true
                columns: root.compact ? 2 : 4
                columnSpacing: 8
                rowSpacing: 8

                Repeater {
                    model: root.feeTierModel

                    Item {
                        id: feeTierOption

                        required property var modelData
                        readonly property string disabledReason: root.feeDisabledReason(modelData)
                        readonly property bool invalid: root.fieldHasError("feeBps")
                                                        && feeTierButton.checked
                        Layout.fillWidth: true
                        implicitHeight: 40

                        Button {
                            id: feeTierButton

                            anchors.fill: parent
                            text: parent.modelData.label || root.feeLabel(parent.modelData.feeBps)
                            checkable: true
                            checked: root.selectedFeeBps === parent.modelData.feeBps
                            enabled: parent.disabledReason.length === 0 && !root.submitting
                            onClicked: root.selectFee(parent.modelData.feeBps)

                            contentItem: Text {
                                text: feeTierButton.text
                                color: feeTierButton.enabled
                                       ? root.theme.colors.textPrimary
                                       : root.theme.colors.textPlaceholder
                                font.pixelSize: 12
                                font.weight: Font.Medium
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }

                            background: Rectangle {
                                radius: 6
                                color: feeTierButton.checked
                                       ? root.theme.colors.selection
                                       : root.theme.colors.inputBg
                                border.color: feeTierOption.invalid
                                              ? root.theme.colors.error
                                              : feeTierButton.checked
                                                ? root.theme.colors.ctaBg
                                                : root.theme.colors.borderStrong
                                border.width: 1
                            }
                        }

                        MouseArea {
                            id: disabledFeeHover
                            anchors.fill: parent
                            enabled: parent.disabledReason.length > 0
                            hoverEnabled: true
                            acceptedButtons: Qt.NoButton
                        }

                        ToolTip.visible: disabledFeeHover.containsMouse
                        ToolTip.text: disabledReason
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            visible: root.activePool

            Text {
                text: qsTr("Slippage")
                color: root.theme.colors.textSecondary
                font.pixelSize: 12
                Layout.fillWidth: true
            }

            Item {
                Layout.preferredWidth: slippageControl.implicitWidth
                Layout.preferredHeight: slippageControl.implicitHeight

                LogosSpinBox {
                    id: slippageControl

                    anchors.fill: parent
                    from: 0
                    to: 5000
                    stepSize: 10
                    editable: true
                    value: root.slippageBps
                    enabled: !root.submitting
                    textFromValue: function(value) { return root.formatBps(value) }
                    valueFromText: function(text) {
                        var parsed = AmountMath.parseHuman(String(text).replace("%", "").trim(), 2)
                        return parsed.ok ? AmountMath.toU32(parsed.raw) : root.slippageBps
                    }
                    onValueModified: {
                        root.slippageBps = value
                        root.noteDraftChanged()
                        root.requestQuote(true)
                    }
                }

                Rectangle {
                    anchors.fill: parent
                    visible: root.fieldHasError("slippageBps")
                    color: "transparent"
                    radius: 6
                    border.color: root.theme.colors.error
                    border.width: 1
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 9
            visible: root.quotePayload.status === "ok"
                     && root.quoteMatchesPair()
                     && !root.quoteStale

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 1
                color: root.theme.colors.divider
            }

            LabelValueRow {
                label: root.activePool ? qsTr("Expected spend") : qsTr("Opening deposit")
                value: root.depositSummary()
            }

            LabelValueRow {
                visible: root.missingPool
                label: qsTr("Initial price")
                value: root.initialPriceText(false)
            }

            LabelValueRow {
                visible: root.missingPool
                label: qsTr("Inverse price")
                value: root.initialPriceText(true)
            }

            LabelValueRow {
                visible: root.missingPool
                label: qsTr("Deposit multiplier")
                value: root.depositMultiplierValue()
            }

            LabelValueRow {
                visible: root.missingPool
                label: qsTr("Deposit scale")
                value: root.depositBasisPointsText()
            }

            LabelValueRow {
                label: qsTr("Expected LP")
                value: root.rawLpText(root.quotePayload.expectedLp)
            }

            LabelValueRow {
                label: root.activePool ? qsTr("Minimum LP") : qsTr("Locked LP")
                value: root.rawLpText(root.activePool
                                      ? root.quotePayload.minimumLp
                                      : root.quotePayload.lockedLp)
            }
        }

        SubmittedTransaction {
            Layout.fillWidth: true
            title: qsTr("Position submitted")
            transactionId: root.transactionId
            visible: root.transactionId.length > 0
        }

        AmmPrimaryButton {
            objectName: "newPositionSubmitButton"
            Layout.fillWidth: true
            Layout.minimumHeight: 56
            theme: root.theme
            text: root.submitting
                  ? qsTr("Submitting…")
                  : root.contextLoading ? qsTr("Loading…")
                  : root.missingPool ? qsTr("Create pool") : qsTr("Add liquidity")
            enabled: root.canConfirm
            onClicked: root.confirmationRequested(root.submissionSnapshot())
        }
    }

    component LabelValueRow: RowLayout {
        required property string label
        required property string value
        property bool valueWrapAnywhere: false
        Layout.fillWidth: true
        spacing: 12

        Text {
            text: parent.label
            color: root.theme.colors.textSecondary
            font.pixelSize: 12
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        Text {
            text: parent.value
            color: root.theme.colors.textPrimary
            font.pixelSize: 12
            font.weight: Font.Medium
            horizontalAlignment: Text.AlignRight
            wrapMode: parent.valueWrapAnywhere ? Text.WrapAnywhere : Text.Wrap
            Layout.maximumWidth: root.compact ? 190 : 280
        }
    }

    component PriceRatioAdjustment: RowLayout {
        id: ratioAdjustment

        required property string amount
        required property string fieldName
        required property bool invalid

        signal edited(string value)

        implicitWidth: 142
        implicitHeight: 24
        spacing: 6

        Text {
            text: qsTr("Price")
            color: root.theme.colors.textSecondary
            font.pixelSize: 11
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 24
            radius: 6
            color: root.theme.colors.panelBg
            border.color: ratioAdjustment.invalid
                          ? root.theme.colors.error
                          : ratioInput.activeFocus
                            ? root.theme.colors.ctaBg
                            : root.theme.colors.borderStrong
            border.width: 1

            TextInput {
                id: ratioInput

                objectName: ratioAdjustment.fieldName
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                text: ratioAdjustment.amount
                enabled: ratioAdjustment.enabled
                color: enabled ? root.theme.colors.textPrimary
                               : root.theme.colors.textSecondary
                font.pixelSize: 12
                selectionColor: root.theme.colors.selection
                selectedTextColor: root.theme.colors.textPrimary
                horizontalAlignment: Text.AlignRight
                verticalAlignment: Text.AlignVCenter
                inputMethodHints: Qt.ImhFormattedNumbersOnly
                maximumLength: 80
                validator: RegularExpressionValidator {
                    regularExpression: /[0-9]*([.][0-9]*)?/
                }
                Accessible.name: qsTr("Initial price ratio amount")
                onTextEdited: ratioAdjustment.edited(text)
            }
        }
    }

    function tokenById(tokenId) {
        for (var i = 0; i < root.tokens.length; ++i) {
            if (root.tokens[i].definitionId === tokenId)
                return root.tokens[i]
        }
        return root.emptyToken
    }

    function selectableTokenIds() {
        // Every injected row is already a valid, selectable token (resolveTokens omits
        // anything unresolvable), so all listed ids are selectable.
        var result = []
        for (var i = 0; i < root.tokens.length; ++i) {
            if (root.tokens[i].definitionId)
                result.push(root.tokens[i].definitionId)
        }
        return result
    }

    function isBase58TokenId(tokenId) {
        return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(String(tokenId || ""))
    }

    function resolveToken(side, value) {
        var tokenId = String(value || "").trim()
        root.tokenResolutionError = ""
        root.tokenResolutionErrorSide = ""
        root.tokenResolutionMessage = ""
        if (!root.isBase58TokenId(tokenId)) {
            root.tokenResolutionError = root.issueText("invalid_token_id")
            root.tokenResolutionErrorSide = side
            return
        }

        // Already in the app's token list → select directly (every listed token is valid).
        var current = root.tokenById(tokenId)
        if (current.definitionId === tokenId) {
            root.selectToken(side, tokenId)
            return
        }
        // A custom/pasted id: ask the app to validate + persist it (backend.addCustomToken),
        // which calls finishTokenResolution(token) on success or failTokenResolution(code) on
        // an unresolvable / non-fungible id.
        root.resolvingTokenId = tokenId
        root.resolvingTokenSide = side
        root.tokenResolveRequested(tokenId)
    }

    function finishTokenResolution(token) {
        if (!root.resolvingToken)
            return
        if (!token || !token.definitionId) {
            root.failTokenResolution("token_definition_unreadable")
            return
        }
        var side = root.resolvingTokenSide
        root.resolvingTokenId = ""
        root.resolvingTokenSide = ""
        root.tokenResolutionMessage = qsTr("%1 - raw supply %2")
                .arg(token.name || root.shortId(token.definitionId))
                .arg(AmountMath.formatRaw(token.totalSupply || "0", 0))
        root.selectToken(side, token.definitionId)
    }

    function failTokenResolution(code) {
        if (!root.resolvingToken)
            return
        var side = root.resolvingTokenSide
        root.resolvingTokenId = ""
        root.resolvingTokenSide = ""
        root.tokenResolutionError = root.issueText(code)
        root.tokenResolutionErrorSide = side
    }

    function reconcileSelection() {
        var previousA = root.selectedTokenAId
        var previousB = root.selectedTokenBId
        var selectable = root.selectableTokenIds()
        if (root.selectedTokenAId.length > 0
                && selectable.indexOf(root.selectedTokenAId) < 0) {
            root.selectedTokenAId = ""
        }
        if (root.selectedTokenBId.length > 0
                && selectable.indexOf(root.selectedTokenBId) < 0) {
            root.selectedTokenBId = ""
        }
        if (root.selectedTokenBId === root.selectedTokenAId)
            root.selectedTokenBId = ""
        if (root.selectedTokenAId !== previousA || root.selectedTokenBId !== previousB)
            root.resetPairDraft()
        else if (root.hasPair)
            root.requestQuote(true)
    }

    function selectToken(side, tokenId) {
        if (tokenId.length === 0)
            return
        root.tokenResolutionError = ""
        root.tokenResolutionErrorSide = ""
        if (side === "A") {
            if (tokenId === root.selectedTokenBId)
                root.selectedTokenBId = root.selectedTokenAId
            root.selectedTokenAId = tokenId
        } else {
            if (tokenId === root.selectedTokenAId)
                root.selectedTokenAId = root.selectedTokenBId
            root.selectedTokenBId = tokenId
        }
        root.resetPairDraft()
    }

    function swapTokens() {
        var tokenId = root.selectedTokenAId
        root.selectedTokenAId = root.selectedTokenBId
        root.selectedTokenBId = tokenId
        var amount = root.amountA
        root.amountA = root.amountB
        root.amountB = amount
        var minimum = root.minimumAmountA
        root.minimumAmountA = root.minimumAmountB
        root.minimumAmountB = minimum
        var priceAmount = root.priceAmountA
        root.priceAmountA = root.priceAmountB
        root.priceAmountB = priceAmount
        root.noteDraftChanged()
        root.requestQuote(true)
    }

    // Back to a first-visit form. resetPairDraft() clears only the draft and keeps
    // the selected pair, which is right when the pair itself just changed; this
    // also drops the pair, the fee tier, slippage and any token-resolution error,
    // for when the form is re-entered rather than edited.
    function resetAll() {
        root.selectedTokenAId = ""
        root.selectedTokenBId = ""
        root.selectedFeeBps = 30
        root.slippageBps = 50
        root.resolvingTokenId = ""
        root.resolvingTokenSide = ""
        root.tokenResolutionError = ""
        root.tokenResolutionErrorSide = ""
        root.tokenResolutionMessage = ""
        root.resetPairDraft()
    }

    function resetPairDraft() {
        root.activePoolQuote = ({})
        root.amountA = ""
        root.amountB = ""
        root.priceAmountA = "1"
        root.priceAmountB = "1"
        root.minimumAmountA = ""
        root.minimumAmountB = ""
        root.localErrors = []
        // The pair changed: the pool is unknown until re-resolved. Reset poolExists BEFORE
        // requestQuote so activePool is false and the empty-amount short-circuit doesn't fire
        // — the probe quote reloads the new pair's reserves/minimum like a fresh selection.
        root.pairReset()
        root.noteDraftChanged()
        root.requestQuote(true)
    }

    function noteDraftChanged() {
        root.draftChanged()
    }

    function rememberActivePoolQuote() {
        if (root.quotePayload.status !== "ok"
                || !root.activePool
                || !root.quoteMatchesPair()) {
            return
        }
        var reserveA = String(root.quotePayload.reserveA || "")
        var reserveB = String(root.quotePayload.reserveB || "")
        if (AmountMath.isUnsigned(reserveA) && reserveA !== "0"
                && AmountMath.isUnsigned(reserveB) && reserveB !== "0") {
            root.activePoolQuote = root.quotePayload
        }
    }

    function knownPoolFeeBps() {
        var direct = root.feeBpsFromQuote(root.quotePayload)
        if (direct > 0)
            return direct
        if (root.quoteMatchesSelectedPair(root.activePoolQuote))
            return Number(root.activePoolQuote.poolFeeBps || 0)
        return 0
    }

    function feeBpsFromQuote(quote) {
        if (!root.quoteMatchesSelectedPair(quote))
            return 0
        var direct = Number(quote.poolFeeBps || 0)
        if (direct > 0)
            return direct
        var errors = quote.errors || []
        for (var i = 0; i < errors.length; ++i) {
            var value = Number(errors[i].details ? errors[i].details.poolFeeBps : 0)
            if (value > 0)
                return value
        }
        return 0
    }

    function quoteMatchesPair() {
        return root.quoteMatchesSelectedPair(root.quotePayload)
    }

    function quoteMatchesSelectedPair(quote) {
        var tokenAId = String(quote.tokenAId || "")
        var tokenBId = String(quote.tokenBId || "")
        return root.hasPair
                && ((tokenAId === root.selectedTokenAId && tokenBId === root.selectedTokenBId)
                    || (tokenAId === root.selectedTokenBId && tokenBId === root.selectedTokenAId))
    }

    function selectFee(feeBps) {
        root.selectedFeeBps = feeBps
        root.noteDraftChanged()
        root.requestQuote(true)
    }

    function feeDisabledReason(tier) {
        if (tier.enabled === false)
            return tier.disabledReason || qsTr("This fee tier is unavailable.")
        if (root.poolFeeBps > 0 && Number(tier.feeBps) !== root.poolFeeBps)
            return qsTr("Existing pool uses %1. Fee tier is fixed for this pair.")
                    .arg(root.feeLabel(root.poolFeeBps))
        return ""
    }

    function feeLabel(feeBps) {
        if (feeBps === 1)
            return "0.01%"
        if (feeBps === 5)
            return "0.05%"
        if (feeBps === 30)
            return "0.30%"
        if (feeBps === 100)
            return "1.00%"
        return root.formatBps(feeBps)
    }

    function buildQuoteRequest() {
        var errors = []
        if (!root.hasPair) {
            errors.push(root.localIssue("token_pair_required", ["tokenAId", "tokenBId"]))
            root.localErrors = errors
            return { "ok": false, "errors": errors, "request": ({}) }
        }

        var request = root.pairRequest()

        if (root.activePool) {
            var parsedA = AmountMath.parseHuman(root.amountA, root.decimalsA)
            var parsedB = AmountMath.parseHuman(root.amountB, root.decimalsB)
            if (!parsedA.ok)
                errors.push(root.localIssue(parsedA.code, ["amountA"]))
            if (!parsedB.ok)
                errors.push(root.localIssue(parsedB.code, ["amountB"]))
            if (errors.length === 0) {
                request.maxAmountA = root.displayIsCanonical ? parsedA.raw : parsedB.raw
                request.maxAmountB = root.displayIsCanonical ? parsedB.raw : parsedA.raw
                request.slippageBps = root.slippageBps
            }
        } else {
            var priceFromAmounts = false
            var price = AmountMath.ratioToQ64(root.priceAmountA,
                                              root.priceAmountB,
                                              root.canonicalDecimalsA,
                                              root.canonicalDecimalsB,
                                              root.displayIsCanonical)
            if (!price.ok)
                errors.push(root.localIssue(price.code, ["initialPrice"]))
            if (root.missingPool && root.minimumAmountA.length > 0) {
                var parsedMissingA = AmountMath.parseHuman(root.amountA, root.decimalsA)
                var parsedMissingB = AmountMath.parseHuman(root.amountB, root.decimalsB)
                if (!parsedMissingA.ok)
                    errors.push(root.localIssue(parsedMissingA.code, ["amountA"]))
                if (!parsedMissingB.ok)
                    errors.push(root.localIssue(parsedMissingB.code, ["amountB"]))
                if (parsedMissingA.ok && parsedMissingB.ok) {
                    if (AmountMath.compare(parsedMissingA.raw, root.minimumAmountA) < 0)
                        errors.push(root.localIssue("amount_too_low", ["amountA"]))
                    if (AmountMath.compare(parsedMissingB.raw, root.minimumAmountB) < 0)
                        errors.push(root.localIssue("amount_too_low", ["amountB"]))
                    var pairedB = AmountMath.pairAmount(parsedMissingA.raw,
                                                        true,
                                                        root.decimalsA,
                                                        root.decimalsB,
                                                        root.priceAmountA,
                                                        root.priceAmountB)
                    var pairedA = AmountMath.pairAmount(parsedMissingB.raw,
                                                        false,
                                                        root.decimalsA,
                                                        root.decimalsB,
                                                        root.priceAmountA,
                                                        root.priceAmountB)
                    var matchesA = pairedA.ok
                            && AmountMath.normalize(pairedA.raw)
                               === AmountMath.normalize(parsedMissingA.raw)
                    var matchesB = pairedB.ok
                            && AmountMath.normalize(pairedB.raw)
                               === AmountMath.normalize(parsedMissingB.raw)
                    if (!matchesA && !matchesB) {
                        errors.push(root.localIssue("deposit_ratio_mismatch", ["amountA", "amountB"]))
                    }
                    if (errors.length === 0) {
                        request.amountA = root.displayIsCanonical
                                ? parsedMissingA.raw : parsedMissingB.raw
                        request.amountB = root.displayIsCanonical
                                ? parsedMissingB.raw : parsedMissingA.raw
                        var actualPrice = AmountMath.ratioToQ64(root.amountA,
                                                               root.amountB,
                                                               root.canonicalDecimalsA,
                                                               root.canonicalDecimalsB,
                                                               root.displayIsCanonical)
                        if (actualPrice.ok) {
                            request.price = actualPrice.raw
                            priceFromAmounts = true
                        } else {
                            errors.push(root.localIssue(actualPrice.code, ["initialPrice"]))
                        }
                    }
                }
            }
            if (price.ok && !priceFromAmounts)
                request.price = price.raw

            if (!root.missingPool) {
                var probeA = root.probeRaw(root.tokenA, root.decimalsA)
                var probeB = root.probeRaw(root.tokenB, root.decimalsB)
                request.maxAmountA = root.displayIsCanonical ? probeA : probeB
                request.maxAmountB = root.displayIsCanonical ? probeB : probeA
                request.slippageBps = root.slippageBps
            }
        }

        root.localErrors = errors
        return { "ok": errors.length === 0, "errors": errors, "request": request }
    }

    function pairRequest() {
        return {
            "tokenAId": root.displayIsCanonical
                        ? root.selectedTokenAId : root.selectedTokenBId,
            "tokenBId": root.displayIsCanonical
                        ? root.selectedTokenBId : root.selectedTokenAId,
            "feeBps": root.selectedFeeBps
        }
    }

    function requestQuote(immediate) {
        if (root.activePool && root.amountA.length === 0 && root.amountB.length === 0) {
            root.localErrors = []
            return
        }
        root.quoteRequested(immediate, root.buildQuoteRequest())
    }

    function probeRaw(token, decimals) {
        var balance = String(token.balance || "0")
        var simulated = AmountMath.multiply(AmountMath.pow10(decimals), "1000")
        if (AmountMath.isUnsigned(balance) && AmountMath.compare(balance, simulated) > 0)
            return balance
        return simulated
    }

    function poolProbeRequest(request) {
        var probe = {}
        for (var field in request)
            probe[field] = request[field]
        var amountA = root.probeRaw(root.tokenA, root.decimalsA)
        var amountB = root.probeRaw(root.tokenB, root.decimalsB)
        probe.maxAmountA = root.displayIsCanonical ? amountA : amountB
        probe.maxAmountB = root.displayIsCanonical ? amountB : amountA
        probe.slippageBps = root.slippageBps
        return probe
    }

    function formatBps(value) {
        return qsTr("%1%").arg(AmountMath.formatRaw(String(value), 2))
    }

    function localIssue(code, fields) {
        return { "code": code, "blockingFields": fields, "details": ({}) }
    }

    function canonicalFieldToDisplay(field) {
        if (field === "tokenAId")
            return root.displayIsCanonical ? "tokenAId" : "tokenBId"
        if (field === "tokenBId")
            return root.displayIsCanonical ? "tokenBId" : "tokenAId"
        if (field === "maxAmountA")
            return root.displayIsCanonical ? "amountA" : "amountB"
        if (field === "maxAmountB")
            return root.displayIsCanonical ? "amountB" : "amountA"
        if (field === "amountA")
            return root.displayIsCanonical ? "amountA" : "amountB"
        if (field === "amountB")
            return root.displayIsCanonical ? "amountB" : "amountA"
        if (field === "price")
            return "initialPrice"
        return field
    }

    // "amount_exceeds_balance" when `side` (A/B)'s entered deposit exceeds its selected holding's
    // balance; "" when no holding is selected, the amount is unparsable, or it fits. Drives
    // fundingSufficient (canConfirm) and the field / form error text.
    function fundingError(side) {
        var holdingId = side === "A" ? root.selectedHoldingAId : root.selectedHoldingBId
        if (holdingId.length === 0)
            return ""
        var amount = side === "A" ? root.amountA : root.amountB
        var decimals = side === "A" ? root.decimalsA : root.decimalsB
        var balanceRaw = side === "A" ? root.selectedBalanceA : root.selectedBalanceB
        var parsed = AmountMath.parseHuman(amount, decimals)
        if (parsed.ok && AmountMath.compare(parsed.raw, balanceRaw) > 0)
            return "amount_exceeds_balance"
        return ""
    }

    function fieldError(field) {
        // Funding is checked independently of the quote — surface it on the offending amount field.
        if (field === "amountA" && root.fundingError("A").length > 0)
            return root.issueText("amount_exceeds_balance")
        if (field === "amountB" && root.fundingError("B").length > 0)
            return root.issueText("amount_exceeds_balance")
        var collections = [root.localErrors, root.currentQuoteErrors()]
        for (var c = 0; c < collections.length; ++c) {
            for (var i = 0; i < collections[c].length; ++i) {
                var fields = collections[c][i].blockingFields || []
                for (var f = 0; f < fields.length; ++f) {
                    if (root.canonicalFieldToDisplay(fields[f]) === field)
                        return root.issueText(collections[c][i].code)
                }
            }
        }
        return ""
    }

    function fieldHasError(field) {
        return root.fieldError(field).length > 0
    }

    function tokenHasError(side) {
        if (root.tokenResolutionError.length > 0
                && root.tokenResolutionErrorSide === side) {
            return true
        }
        return root.fieldHasError(side === "A" ? "tokenAId" : "tokenBId")
    }

    function formErrorText() {
        if (root.tokenResolutionError.length > 0)
            return root.tokenResolutionError
        if (root.submitError.length > 0)
            return root.submitError
        if (root.fundingError("A").length > 0 || root.fundingError("B").length > 0)
            return root.issueText("amount_exceeds_balance")
        var collections = [root.localErrors, root.currentQuoteErrors()]
        for (var c = 0; c < collections.length; ++c) {
            for (var i = 0; i < collections[c].length; ++i) {
                var code = String(collections[c][i].code || "")
                if (code.length > 0)
                    return root.issueText(code)
            }
        }
        return root.quoteError()
    }

    function currentQuoteErrors() {
        return !root.quoteStale && root.quoteMatchesPair()
                ? root.quotePayload.errors || [] : []
    }

    function issueText(code) {
        var messages = {
            "amount_required": qsTr("Enter a value."),
            "amount_must_be_positive": qsTr("Value must be greater than zero."),
            "invalid_amount_format": qsTr("Use plain dot-decimal format."),
            "invalid_amount_precision": qsTr("Token amounts must use whole raw units."),
            "invalid_raw_amount": qsTr("Value is outside the supported range."),
            "amount_exceeds_balance": qsTr("Amount exceeds the selected holding balance."),
            "amount_too_low": qsTr("Value is too low for this pool."),
            "invalid_token_id": qsTr("Enter a valid base58 TokenDefinition ID."),
            "deposit_ratio_mismatch": qsTr("Deposit amounts must match the initial price."),
            "minimum_lp_zero": qsTr("Slippage leaves no minimum LP output."),
            "invalid_slippage": qsTr("Slippage must be between 0% and 50%."),
            "fee_tier_mismatch": qsTr("Select the existing pool fee tier."),
            "no_wallet": qsTr("Connect a wallet to submit this position."),
            "wallet_unavailable": qsTr("Wallet is unavailable."),
            "wallet_submission_failed": qsTr("Wallet submission failed. Review and retry manually."),
            "signature_rejected": qsTr("Wallet approval was rejected."),
            "submit_in_progress": qsTr("A submission is already in progress."),
            "transaction_deadline_expired": qsTr("Wallet approval expired. Retry to create a fresh request."),
            "high_slippage": qsTr("High slippage tolerance."),
            "token_definition_unreadable": qsTr("A token definition could not be read."),
            "token_program_mismatch": qsTr("Token belongs to a different TokenProgram."),
            "token_not_fungible": qsTr("Token is not a public fungible token."),
            "backend_error": qsTr("Position backend failed. Refresh and retry."),
            "config_missing": qsTr("Network configuration is missing."),
            "account_read_failed": qsTr("Required on-chain state could not be read."),
            "pool_unavailable": qsTr("Pool state is unavailable."),
            "config_unavailable": qsTr("AMM configuration is unavailable."),
            "same_token_pair": qsTr("Select two different tokens."),
            "token_pair_required": qsTr("Select two tokens.")
        }
        return messages[code] || qsTr("Position quote is unavailable (%1).").arg(code || "unknown")
    }

    function editActiveAmount(side, value) {
        if (side === "A")
            root.amountA = value
        else
            root.amountB = value

        root.localErrors = []
        root.noteDraftChanged()
    }

    function finishActiveAmount(side, value) {
        var decimals = side === "A" ? root.decimalsA : root.decimalsB
        if (side === "A")
            root.amountA = value
        else
            root.amountB = value

        var reserveA = root.poolReserve("A")
        var reserveB = root.poolReserve("B")
        var parsed = AmountMath.parseHuman(value, decimals)
        if (parsed.ok && reserveA !== "" && reserveB !== ""
                && reserveA !== "0" && reserveB !== "0") {
            if (side === "A") {
                var rawB = AmountMath.mulDivFloor(parsed.raw, reserveB, reserveA)
                root.amountB = AmountMath.formatRaw(rawB, root.decimalsB)
            } else {
                var rawA = AmountMath.mulDivFloor(parsed.raw, reserveA, reserveB)
                root.amountA = AmountMath.formatRaw(rawA, root.decimalsA)
            }
        }
        root.requestQuote(true)
    }

    function useMaximum() {
        var reserveA = root.poolReserve("A")
        var reserveB = root.poolReserve("B")
        if (!reserveA || !reserveB || reserveA === "0" || reserveB === "0")
            return
        var balanceA = String(root.tokenA.balance || "0")
        var balanceB = String(root.tokenB.balance || "0")
        var fitA = AmountMath.mulDivFloor(balanceB, reserveA, reserveB)
        var rawA = AmountMath.compare(balanceA, fitA) < 0 ? balanceA : fitA
        var rawB = AmountMath.mulDivFloor(rawA, reserveB, reserveA)
        root.amountA = AmountMath.formatRaw(rawA, root.decimalsA)
        root.amountB = AmountMath.formatRaw(rawB, root.decimalsB)
        root.noteDraftChanged()
        root.requestQuote(true)
    }

    function editPrice(side, value) {
        if (side === "A")
            root.priceAmountA = value
        else
            root.priceAmountB = value
        root.minimumAmountA = ""
        root.minimumAmountB = ""
        root.amountA = ""
        root.amountB = ""
        root.noteDraftChanged()
        root.requestQuote(false)
    }

    function editMissingAmount(side, value) {
        if (side === "A")
            root.amountA = value
        else
            root.amountB = value

        root.localErrors = []
        root.noteDraftChanged()
    }

    function finishMissingAmount(side, value) {
        var decimals = side === "A" ? root.decimalsA : root.decimalsB
        if (side === "A")
            root.amountA = value
        else
            root.amountB = value

        var parsed = AmountMath.parseHuman(value, decimals)
        if (parsed.ok) {
            var paired = AmountMath.pairAmount(parsed.raw,
                                               side === "A",
                                               root.decimalsA,
                                               root.decimalsB,
                                               root.priceAmountA,
                                               root.priceAmountB)
            if (paired.ok) {
                if (side === "A")
                    root.amountB = AmountMath.formatRaw(paired.raw, root.decimalsB)
                else
                    root.amountA = AmountMath.formatRaw(paired.raw, root.decimalsA)
            }
        }
        root.requestQuote(true)
    }

    function applyQuoteSideEffects() {
        if (root.quoteStale)
            return
        if (root.poolFeeBps > 0 && root.selectedFeeBps !== root.poolFeeBps) {
            root.selectedFeeBps = root.poolFeeBps
            root.localErrors = []
            root.quoteRequested(true, {
                "ok": true,
                "errors": [],
                "request": root.poolProbeRequest(root.pairRequest())
            })
            return
        }

        if (root.quotePayload.status !== "ok")
            return

        if (root.missingPool) {
            var rawA = root.displayRaw("actualAmountA", "actualAmountB", "A")
            var rawB = root.displayRaw("actualAmountA", "actualAmountB", "B")
            var minimumA = root.displayRaw("minimumAmountA", "minimumAmountB", "A")
            var minimumB = root.displayRaw("minimumAmountA", "minimumAmountB", "B")
            root.minimumAmountA = minimumA.length > 0 ? minimumA : rawA
            root.minimumAmountB = minimumB.length > 0 ? minimumB : rawB
            if (rawA.length > 0 && rawB.length > 0) {
                root.amountA = AmountMath.formatRaw(rawA, root.decimalsA)
                root.amountB = AmountMath.formatRaw(rawB, root.decimalsB)
            }
        }
    }

    function displayRaw(canonicalAField, canonicalBField, side) {
        return root.displayQuoteRaw(root.quotePayload, canonicalAField, canonicalBField, side)
    }

    function displayQuoteRaw(quote, canonicalAField, canonicalBField, side) {
        if (!root.quoteMatchesSelectedPair(quote))
            return ""
        if (side === "A")
            return String(quote[root.displayIsCanonical ? canonicalAField : canonicalBField] || "")
        return String(quote[root.displayIsCanonical ? canonicalBField : canonicalAField] || "")
    }

    function poolReserve(side) {
        var reserve = root.displayRaw("reserveA", "reserveB", side)
        if (AmountMath.isUnsigned(reserve) && reserve !== "0")
            return reserve
        if (!root.quoteMatchesSelectedPair(root.activePoolQuote))
            return ""
        return root.displayQuoteRaw(root.activePoolQuote, "reserveA", "reserveB", side)
    }

    function quoteAmount(canonicalAField, canonicalBField, side) {
        var token = side === "A" ? root.tokenA : root.tokenB
        var decimals = side === "A" ? root.decimalsA : root.decimalsB
        var raw = root.displayRaw(canonicalAField, canonicalBField, side)
        return raw.length > 0
                ? qsTr("%1 %2").arg(AmountMath.formatRaw(raw, decimals)).arg(root.shortTokenName(token))
                : "—"
    }

    function depositSummary() {
        var amountA = root.quoteAmount("actualAmountA", "actualAmountB", "A")
        var amountB = root.quoteAmount("actualAmountA", "actualAmountB", "B")
        return amountA + " + " + amountB
    }

    function initialPriceText(inverse) {
        var price = inverse ? root.inverseInitialPrice : root.initialPrice
        if (price.length === 0)
            return "—"
        var from = inverse ? root.tokenB : root.tokenA
        var to = inverse ? root.tokenA : root.tokenB
        return qsTr("1 %1 = %2 %3")
                .arg(root.shortTokenName(from))
                .arg(price)
                .arg(root.shortTokenName(to))
    }

    function rawLpText(raw) {
        return raw !== undefined && raw !== null && String(raw).length > 0
                ? qsTr("%1 raw LP").arg(String(raw)) : "—"
    }

    function activePriceValue() {
        var price = String(root.quotePayload.price || "")
        if (price.length === 0 && root.quoteMatchesSelectedPair(root.activePoolQuote))
            price = String(root.activePoolQuote.price || "")
        return AmountMath.priceFromQ64(price,
                                       root.canonicalDecimalsA,
                                       root.canonicalDecimalsB,
                                       root.displayIsCanonical)
    }

    function activePriceText() {
        var price = root.activePriceValue()
        if (price.length === 0)
            return ""
        return qsTr("1 %1 = %2 %3")
                .arg(root.shortTokenName(root.tokenA))
                .arg(price)
                .arg(root.shortTokenName(root.tokenB))
    }

    function quoteError() {
        if (root.quoteLoading || root.quoteStale)
            return ""
        if (root.quotePayload.status === "error")
            return root.issueText(root.quotePayload.code)
        return ""
    }

    function submissionSnapshot() {
        var built = root.buildQuoteRequest()
        return {
            "request": built.request,
            // Canonical-order holdings for the createPool / addLiquidity calls: the
            // request's tokenAId/amountA are canonical, so holdingAId must be the
            // canonical token A's holding too (the module re-canonicalizes as a no-op).
            // The user picks these via the per-side account selectors; selectedHoldingA
            // is display token A's holding, so it aligns with tokenA the same way.
            "holdingAId": String(root.displayIsCanonical ? root.selectedHoldingAId : root.selectedHoldingBId),
            "holdingBId": String(root.displayIsCanonical ? root.selectedHoldingBId : root.selectedHoldingAId),
            // The add path's slippage floor on the LP minted (orientation-independent),
            // taken from the active-pool quote; ignored by the create path.
            "minLp": String(root.quotePayload.minimumLp || ""),
            "pairText": qsTr("%1 / %2").arg(root.shortTokenName(root.tokenA)).arg(root.shortTokenName(root.tokenB)),
            "feeText": root.feeLabel(root.selectedFeeBps),
            "depositAText": root.quoteAmount("actualAmountA", "actualAmountB", "A"),
            "depositBText": root.quoteAmount("actualAmountA", "actualAmountB", "B"),
            "expectedLpText": root.rawLpText(root.quotePayload.expectedLp),
            // Confirm-dialog action derives from the resolved pool state (add vs create).
            "poolExists": root.activePool
        }
    }

    function shortTokenName(token) {
        if (token && token.name && token.name.length > 0)
            return token.name
        return token && token.definitionId ? root.shortId(token.definitionId) : "—"
    }

    function balanceText(token, decimals) {
        return AmountMath.formatRaw(String(token.balance || "0"), decimals)
    }

    function tokenBalanceDetail(token) {
        return qsTr("Available %1").arg(root.balanceText(token, 0))
    }

    function shortId(value) {
        var text = String(value || "")
        return text.length > 14 ? text.slice(0, 7) + "…" + text.slice(-5) : text
    }

    function depositMultiplierText() {
        var multiplier = root.depositMultiplierValue()
        var basisPoints = root.depositBasisPointsText()
        return multiplier.length > 0 ? qsTr("%1 · %2").arg(multiplier).arg(basisPoints) : ""
    }

    function depositMultiplierValue() {
        var scale = root.depositScaleValue()
        return scale.length > 0
                ? qsTr("%1x minimum").arg(AmountMath.formatRaw(scale, 4)) : ""
    }

    function depositBasisPointsText() {
        var scale = root.depositScaleValue()
        return scale.length > 0 ? qsTr("%1 basis points").arg(scale) : ""
    }

    function depositScaleValue() {
        var parsed = AmountMath.parseHuman(root.amountA, root.decimalsA)
        if (!parsed.ok || !AmountMath.isUnsigned(root.minimumAmountA)
                || root.minimumAmountA === "0"
                || AmountMath.compare(parsed.raw, root.minimumAmountA) < 0) {
            return ""
        }
        return AmountMath.divide(AmountMath.multiply(parsed.raw, "10000"),
                                 root.minimumAmountA).quotient
    }

    function minimumAmountText(side) {
        var raw = side === "A" ? root.minimumAmountA : root.minimumAmountB
        var decimals = side === "A" ? root.decimalsA : root.decimalsB
        return raw.length > 0
                ? qsTr("Min %1").arg(AmountMath.formatRaw(raw, decimals)) : ""
    }

    function contextStatusText() {
        return root.walletReady ? qsTr("Wallet ready")
                                : qsTr("Wallet disconnected · simulation only")
    }
}
