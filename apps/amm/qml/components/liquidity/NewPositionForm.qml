pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Controls
import Logos.Icons
import Logos.Theme

import "AmountMath.js" as AmountMath
import "../wallet"

Rectangle {
    id: root

    property var newPositionContext: ({})
    property var quotePayload: ({})
    property bool contextLoading: false
    property bool quoteLoading: false
    property bool submitting: false
    property bool quoteStale: false
    property string selectedTokenAId: ""
    property string selectedTokenBId: ""
    property int selectedFeeBps: 30
    property int slippageBps: 50
    property string amountA: ""
    property string amountB: ""
    property string initialPrice: "1"
    property string depositScaleBps: "10000"
    property string submitError: ""
    property string transactionId: ""
    property string refreshWarning: ""
    property var localErrors: []
    property string priceTargetLp: ""
    property int priceAdjustmentCount: 0

    readonly property var emptyToken: ({
        "definitionId": "",
        "name": "",
        "totalSupplyRaw": "0",
        "balanceRaw": "0",
        "selectable": false
    })
    readonly property var tokens: root.newPositionContext && root.newPositionContext.tokens
                                         ? root.newPositionContext.tokens : []
    readonly property var feeTiers: root.newPositionContext && root.newPositionContext.feeTiers
                                           ? root.newPositionContext.feeTiers : []
    readonly property var tokenA: root.tokenById(root.selectedTokenAId)
    readonly property var tokenB: root.tokenById(root.selectedTokenBId)
    readonly property int decimalsA: AmountMath.implyDecimals(root.tokenA.totalSupplyRaw || "0")
    readonly property int decimalsB: AmountMath.implyDecimals(root.tokenB.totalSupplyRaw || "0")
    readonly property bool displayIsCanonical: root.selectedTokenAId.length > 0
                                               && AmountMath.compareBase58Ids(root.selectedTokenAId,
                                                                              root.selectedTokenBId) > 0
    readonly property var canonicalTokenA: root.displayIsCanonical ? root.tokenA : root.tokenB
    readonly property var canonicalTokenB: root.displayIsCanonical ? root.tokenB : root.tokenA
    readonly property int canonicalDecimalsA: root.displayIsCanonical ? root.decimalsA : root.decimalsB
    readonly property int canonicalDecimalsB: root.displayIsCanonical ? root.decimalsB : root.decimalsA
    readonly property string poolStatus: root.effectivePoolStatus()
    readonly property bool activePool: root.poolStatus === "active_pool"
    readonly property bool missingPool: root.poolStatus === "missing_pool"
    readonly property int poolFeeBps: root.knownPoolFeeBps()
    readonly property bool compact: root.width < 640
    readonly property bool hasPair: root.selectedTokenAId.length > 0
                                    && root.selectedTokenBId.length > 0
                                    && root.selectedTokenAId !== root.selectedTokenBId
    readonly property bool canConfirm: root.quotePayload.schema === "new-position.v1"
                                       && root.quotePayload.status === "ok"
                                       && root.quotePayload.canSubmit === true
                                       && String(root.quotePayload.quoteHash || "").length > 0
                                       && !root.contextLoading
                                       && !root.quoteLoading
                                       && !root.quoteStale
                                       && !root.submitting

    signal quoteRequested(bool immediate)
    signal confirmationRequested(var snapshot)
    signal tokenResolveRequested(string tokenId)
    signal draftChanged
    signal refreshRequested

    implicitHeight: content.implicitHeight + 48
    implicitWidth: 760
    radius: 8
    color: Theme.palette.backgroundSecondary
    border.color: Theme.palette.borderSecondary
    border.width: 1

    Component.onCompleted: Qt.callLater(root.ensurePair)
    onNewPositionContextChanged: Qt.callLater(root.ensurePair)
    onQuotePayloadChanged: Qt.callLater(root.applyQuoteSideEffects)

    TextEdit {
        id: clipboardProxy
        visible: false
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: 24
        spacing: 18

        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 3

                Text {
                    text: qsTr("New position")
                    color: Theme.palette.text
                    font.pixelSize: 22
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0
                }

                Text {
                    text: root.contextStatusText()
                    color: Theme.palette.textSecondary
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

            WalletIconButton {
                iconSource: LogosIcons.refresh
                iconColor: Theme.palette.textSecondary
                iconSize: 18
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                enabled: !root.contextLoading && !root.submitting
                Accessible.name: qsTr("Refresh position data")
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                onClicked: root.refreshRequested()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: networkMessage.implicitHeight + 20
            radius: 6
            color: Theme.palette.backgroundTertiary
            border.color: Theme.palette.error
            visible: root.contextBlocksForm()

            Text {
                id: networkMessage
                anchors.fill: parent
                anchors.margins: 10
                text: root.contextErrorText()
                color: Theme.palette.text
                font.pixelSize: 12
                wrapMode: Text.Wrap
                verticalAlignment: Text.AlignVCenter
            }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: root.compact ? 1 : 3
            columnSpacing: 10
            rowSpacing: 8

            LogosComboBox {
                id: tokenASelector

                Layout.fillWidth: true
                Layout.minimumHeight: 48
                model: root.tokens
                currentIndex: root.tokenIndex(root.selectedTokenAId)
                displayText: root.tokenLabel(root.tokenA)
                enabled: root.tokens.length > 0 && !root.submitting

                onActivated: function(index) {
                    root.selectToken("A", root.tokens[index].definitionId)
                }

                delegate: ItemDelegate {
                    required property var modelData
                    width: tokenASelector.width
                    enabled: modelData.selectable === true
                    text: root.tokenLabel(modelData)
                    ToolTip.visible: hovered
                    ToolTip.text: root.tokenDetail(modelData)
                }
            }

            Button {
                text: "↔"
                enabled: root.hasPair && !root.submitting
                flat: true
                font.pixelSize: 20
                Accessible.name: qsTr("Swap token order")
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                Layout.alignment: Qt.AlignCenter
                Layout.preferredWidth: 44
                Layout.preferredHeight: 44
                onClicked: root.swapTokens()

                contentItem: Text {
                    text: parent.text
                    color: parent.enabled ? Theme.palette.text : Theme.palette.textMuted
                    font.pixelSize: 20
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    radius: 6
                    color: Theme.palette.backgroundButton
                    border.color: Theme.palette.border
                    border.width: 1
                }
            }

            LogosComboBox {
                id: tokenBSelector

                Layout.fillWidth: true
                Layout.minimumHeight: 48
                model: root.tokens
                currentIndex: root.tokenIndex(root.selectedTokenBId)
                displayText: root.tokenLabel(root.tokenB)
                enabled: root.tokens.length > 0 && !root.submitting

                onActivated: function(index) {
                    root.selectToken("B", root.tokens[index].definitionId)
                }

                delegate: ItemDelegate {
                    required property var modelData
                    width: tokenBSelector.width
                    enabled: modelData.selectable === true
                    text: root.tokenLabel(modelData)
                    ToolTip.visible: hovered
                    ToolTip.text: root.tokenDetail(modelData)
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            LogosTextField {
                id: tokenIdField
                Layout.fillWidth: true
                placeholderText: qsTr("Token definition ID")
                enabled: !root.contextLoading && !root.submitting
                textInput.maximumLength: 64
                textInput.selectByMouse: true
            }

            LogosButton {
                id: addTokenButton
                text: qsTr("Add")
                enabled: tokenIdField.text.length > 0 && !root.contextLoading && !root.submitting
                Layout.preferredWidth: 80
                Layout.preferredHeight: 40
                radius: 6
                onClicked: {
                    root.tokenResolveRequested(tokenIdField.text)
                    tokenIdField.text = ""
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: Theme.palette.borderSecondary
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8

            Text {
                text: qsTr("Fee tier")
                color: Theme.palette.text
                font.pixelSize: 13
                font.weight: Font.Medium
            }

            GridLayout {
                Layout.fillWidth: true
                columns: root.compact ? 2 : 4
                columnSpacing: 8
                rowSpacing: 8

                Repeater {
                    model: root.feeTiers

                    Item {
                        required property var modelData
                        readonly property string disabledReason: root.feeDisabledReason(modelData)
                        Layout.fillWidth: true
                        implicitHeight: 40

                        Button {
                            anchors.fill: parent
                            text: parent.modelData.label || root.feeLabel(parent.modelData.feeBps)
                            checkable: true
                            checked: root.selectedFeeBps === parent.modelData.feeBps
                            enabled: parent.disabledReason.length === 0 && !root.submitting
                            onClicked: root.selectFee(parent.modelData.feeBps)

                            contentItem: Text {
                                text: parent.text
                                color: parent.enabled ? Theme.palette.text : Theme.palette.textMuted
                                font.pixelSize: 12
                                font.weight: Font.Medium
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }

                            background: Rectangle {
                                radius: 6
                                color: parent.checked
                                       ? Theme.palette.overlayOrange
                                       : Theme.palette.backgroundButton
                                border.color: parent.checked
                                              ? Theme.palette.primary
                                              : Theme.palette.border
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

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: root.activePool

            RowLayout {
                Layout.fillWidth: true

                Text {
                    text: qsTr("Deposit amounts")
                    color: Theme.palette.text
                    font.pixelSize: 13
                    font.weight: Font.Medium
                    Layout.fillWidth: true
                }

                Text {
                    text: root.activePriceText()
                    color: Theme.palette.textSecondary
                    font.pixelSize: 12
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignRight
                    Layout.maximumWidth: root.compact ? 180 : 360
                }
            }

            GridLayout {
                Layout.fillWidth: true
                columns: root.compact ? 1 : 2
                columnSpacing: 10
                rowSpacing: 10

                TokenAmountInput {
                    Layout.fillWidth: true
                    text: root.amountA
                    label: root.tokenA.name || qsTr("Token A")
                    token: root.shortTokenName(root.tokenA)
                    balance: root.balanceText(root.tokenA, root.decimalsA)
                    errorText: root.fieldError("amountA")
                    readOnly: root.submitting
                    onEditingChanged: function(value) { root.editActiveAmount("A", value) }
                    onMaxClicked: root.useMaximum()
                }

                TokenAmountInput {
                    Layout.fillWidth: true
                    text: root.amountB
                    label: root.tokenB.name || qsTr("Token B")
                    token: root.shortTokenName(root.tokenB)
                    balance: root.balanceText(root.tokenB, root.decimalsB)
                    errorText: root.fieldError("amountB")
                    readOnly: root.submitting
                    onEditingChanged: function(value) { root.editActiveAmount("B", value) }
                    onMaxClicked: root.useMaximum()
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Text {
                    text: qsTr("Slippage")
                    color: Theme.palette.textSecondary
                    font.pixelSize: 12
                    Layout.fillWidth: true
                }

                LogosSpinBox {
                    from: 0
                    to: 5000
                    stepSize: 10
                    editable: true
                    value: root.slippageBps
                    enabled: !root.submitting
                    textFromValue: function(value) { return qsTr("%1 bps").arg(value) }
                    valueFromText: function(text) {
                        var parsed = Number(String(text).replace(/[^0-9]/g, ""))
                        return isNaN(parsed) ? root.slippageBps : parsed
                    }
                    onValueModified: {
                        root.slippageBps = value
                        root.noteDraftChanged()
                        root.quoteRequested(true)
                    }
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: root.missingPool

            Text {
                text: qsTr("Initial price")
                color: Theme.palette.text
                font.pixelSize: 13
                font.weight: Font.Medium
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: qsTr("1 %1 =").arg(root.shortTokenName(root.tokenA))
                    color: Theme.palette.textSecondary
                    font.pixelSize: 13
                }

                LogosTextField {
                    Layout.fillWidth: true
                    text: root.initialPrice
                    enabled: !root.submitting
                    textInput.inputMethodHints: Qt.ImhFormattedNumbersOnly
                    textInput.maximumLength: 80
                    textInput.validator: RegularExpressionValidator { regularExpression: /[0-9]*([.][0-9]*)?/ }
                    textInput.onTextEdited: root.editPrice(textInput.text)
                }

                Text {
                    text: root.shortTokenName(root.tokenB)
                    color: Theme.palette.textSecondary
                    font.pixelSize: 13
                }
            }

            Text {
                text: root.fieldError("initialPrice")
                color: Theme.palette.error
                font.pixelSize: 11
                visible: text.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: qsTr("Deposit scale")
                    color: Theme.palette.textSecondary
                    font.pixelSize: 12
                    Layout.fillWidth: true
                }

                Button {
                    text: "−"
                    enabled: Number(root.depositScaleBps) > 10000 && !root.submitting
                    Accessible.name: qsTr("Decrease deposit scale")
                    implicitWidth: 36
                    onClicked: root.stepScale(-100)

                    contentItem: Text {
                        text: parent.text
                        color: parent.enabled ? Theme.palette.text : Theme.palette.textMuted
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }

                    background: Rectangle {
                        radius: 6
                        color: Theme.palette.backgroundButton
                        border.color: Theme.palette.border
                        border.width: 1
                    }
                }

                LogosTextField {
                    id: scaleField
                    text: root.depositScaleBps
                    enabled: !root.submitting
                    textInput.horizontalAlignment: Text.AlignHCenter
                    textInput.maximumLength: 10
                    textInput.validator: RegularExpressionValidator { regularExpression: /[0-9]{0,10}/ }
                    textInput.inputMethodHints: Qt.ImhDigitsOnly
                    Layout.preferredWidth: 116
                    textInput.onTextEdited: root.editScale(textInput.text)
                }

                Text {
                    text: qsTr("bps")
                    color: Theme.palette.textSecondary
                    font.pixelSize: 12
                }

                Button {
                    text: "+"
                    enabled: Number(root.depositScaleBps) <= 4294967195 && !root.submitting
                    Accessible.name: qsTr("Increase deposit scale")
                    implicitWidth: 36
                    onClicked: root.stepScale(100)

                    contentItem: Text {
                        text: parent.text
                        color: parent.enabled ? Theme.palette.text : Theme.palette.textMuted
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }

                    background: Rectangle {
                        radius: 6
                        color: Theme.palette.backgroundButton
                        border.color: Theme.palette.border
                        border.width: 1
                    }
                }
            }

            Text {
                text: root.scaleHelpText()
                color: root.fieldError("depositScale").length > 0 ? Theme.palette.error : Theme.palette.textSecondary
                font.pixelSize: 11
                visible: text.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: missingDepositRows.implicitHeight + 20
                radius: 6
                color: Theme.palette.backgroundTertiary

                ColumnLayout {
                    id: missingDepositRows
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 8

                    LabelValueRow {
                        label: qsTr("%1 deposit").arg(root.shortTokenName(root.tokenA))
                        value: root.quoteAmount("actualAmountARaw", "actualAmountBRaw", "A")
                    }

                    LabelValueRow {
                        label: qsTr("%1 deposit").arg(root.shortTokenName(root.tokenB))
                        value: root.quoteAmount("actualAmountARaw", "actualAmountBRaw", "B")
                    }
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 9
            visible: root.quotePayload.status === "ok"

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 1
                color: Theme.palette.borderSecondary
            }

            LabelValueRow {
                label: root.activePool ? qsTr("Expected spend") : qsTr("Opening deposit")
                value: root.depositSummary()
            }

            LabelValueRow {
                label: qsTr("Expected LP")
                value: root.rawLpText(root.quotePayload.expectedLpRaw)
            }

            LabelValueRow {
                label: root.activePool ? qsTr("Minimum LP") : qsTr("Locked LP")
                value: root.rawLpText(root.activePool
                                      ? root.quotePayload.minimumLpRaw
                                      : root.quotePayload.lockedLpRaw)
            }

            LabelValueRow {
                label: qsTr("Pool")
                value: String(root.quotePayload.poolId || "")
            }

            LogosButton {
                id: accountPlanButton
                text: qsTr("Account plan (%1)").arg(root.accountPreview().length)
                enabled: root.accountPreview().length > 0
                property bool checked: false
                implicitWidth: 150
                implicitHeight: 36
                radius: 6
                Layout.alignment: Qt.AlignLeft
                onClicked: checked = !checked
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 5
                visible: accountPlanButton.checked

                Repeater {
                    model: root.accountPreview()

                    LabelValueRow {
                        required property var modelData
                        label: qsTr("%1. %2 · %3").arg(modelData.order + 1).arg(modelData.role).arg(modelData.action)
                        value: modelData.accountId ? modelData.accountId : qsTr("Assigned by wallet")
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: warningTextItem.implicitHeight + 20
            radius: 6
            color: Theme.palette.backgroundTertiary
            border.color: Theme.palette.primary
            visible: root.warningText().length > 0

            Text {
                id: warningTextItem
                anchors.fill: parent
                anchors.margins: 10
                text: root.warningText()
                color: Theme.palette.text
                font.pixelSize: 12
                wrapMode: Text.Wrap
                verticalAlignment: Text.AlignVCenter
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: quoteErrorText.implicitHeight + 20
            radius: 6
            color: Theme.palette.backgroundTertiary
            border.color: Theme.palette.error
            visible: root.quoteError().length > 0

            Text {
                id: quoteErrorText
                anchors.fill: parent
                anchors.margins: 10
                text: root.quoteError()
                color: Theme.palette.text
                font.pixelSize: 12
                wrapMode: Text.Wrap
                verticalAlignment: Text.AlignVCenter
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: successColumn.implicitHeight + 24
            radius: 6
            color: Theme.palette.backgroundTertiary
            border.color: Theme.palette.success
            visible: root.transactionId.length > 0

            ColumnLayout {
                id: successColumn
                anchors.fill: parent
                anchors.margins: 12
                spacing: 7

                Text {
                    text: qsTr("Position submitted")
                    color: Theme.palette.success
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6

                    Text {
                        text: root.transactionId
                        color: Theme.palette.text
                        font.family: "monospace"
                        font.pixelSize: 11
                        wrapMode: Text.WrapAnywhere
                        Layout.fillWidth: true
                    }

                    LogosCopyButton {
                        Accessible.name: qsTr("Copy transaction ID")
                        ToolTip.visible: hovered
                        ToolTip.text: Accessible.name
                        onCopyText: root.copyToClipboard(root.transactionId)
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    visible: root.refreshWarning.length > 0

                    Text {
                        text: root.refreshWarning
                        color: Theme.palette.textSecondary
                        font.pixelSize: 11
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }

                    WalletIconButton {
                        iconSource: LogosIcons.refresh
                        iconColor: Theme.palette.textSecondary
                        iconSize: 16
                        Accessible.name: qsTr("Refresh balances")
                        ToolTip.visible: hovered
                        ToolTip.text: Accessible.name
                        onClicked: root.refreshRequested()
                    }
                }
            }
        }

        Text {
            text: root.submitError
            color: Theme.palette.error
            font.pixelSize: 12
            visible: text.length > 0
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        LogosButton {
            Layout.fillWidth: true
            Layout.minimumHeight: 48
            radius: 6
            text: root.submitting
                  ? qsTr("Submitting…")
                  : root.missingPool ? qsTr("Create pool") : qsTr("Add liquidity")
            enabled: root.canConfirm
            onClicked: root.confirmationRequested(root.submissionSnapshot())
        }
    }

    component LabelValueRow: RowLayout {
        required property string label
        required property string value
        Layout.fillWidth: true
        spacing: 12

        Text {
            text: parent.label
            color: Theme.palette.textSecondary
            font.pixelSize: 12
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        Text {
            text: parent.value
            color: Theme.palette.text
            font.pixelSize: 12
            font.weight: Font.Medium
            horizontalAlignment: Text.AlignRight
            wrapMode: Text.WrapAnywhere
            Layout.maximumWidth: root.compact ? 190 : 430
        }
    }

    function tokenById(tokenId) {
        for (var i = 0; i < root.tokens.length; ++i) {
            if (root.tokens[i].definitionId === tokenId)
                return root.tokens[i]
        }
        return root.emptyToken
    }

    function tokenIndex(tokenId) {
        for (var i = 0; i < root.tokens.length; ++i) {
            if (root.tokens[i].definitionId === tokenId)
                return i
        }
        return -1
    }

    function selectableTokenIds() {
        var result = []
        for (var i = 0; i < root.tokens.length; ++i) {
            if (root.tokens[i].selectable === true)
                result.push(root.tokens[i].definitionId)
        }
        return result
    }

    function ensurePair() {
        var selectable = root.selectableTokenIds()
        if (selectable.length === 0) {
            root.selectedTokenAId = ""
            root.selectedTokenBId = ""
            return
        }
        if (selectable.indexOf(root.selectedTokenAId) < 0)
            root.selectedTokenAId = selectable[0]
        if (selectable.indexOf(root.selectedTokenBId) < 0
                || root.selectedTokenBId === root.selectedTokenAId) {
            root.selectedTokenBId = selectable.length > 1 ? selectable[1] : ""
        }
        if (root.hasPair)
            root.quoteRequested(true)
    }

    function selectToken(side, tokenId) {
        if (tokenId.length === 0)
            return
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
        if (root.activePool && root.quotePayload.initialPriceRealRaw)
            root.initialPrice = root.activePriceValue()
        root.noteDraftChanged()
        root.quoteRequested(true)
    }

    function resetPairDraft() {
        root.amountA = ""
        root.amountB = ""
        root.initialPrice = "1"
        root.depositScaleBps = "10000"
        root.priceTargetLp = ""
        root.priceAdjustmentCount = 0
        root.localErrors = []
        root.noteDraftChanged()
        root.quoteRequested(true)
    }

    function noteDraftChanged() {
        root.submitError = ""
        root.refreshWarning = ""
        root.draftChanged()
    }

    function effectivePoolStatus() {
        var status = String(root.quotePayload.poolStatus || "")
        if (status === "active_pool" || status === "missing_pool")
            return status
        if (root.quotePayload.code === "fee_tier_mismatch")
            return "active_pool"
        return ""
    }

    function knownPoolFeeBps() {
        var direct = Number(root.quotePayload.poolFeeBps || 0)
        if (direct > 0)
            return direct
        var errors = root.quotePayload.errors || []
        for (var i = 0; i < errors.length; ++i) {
            var value = Number(errors[i].details ? errors[i].details.poolFeeBps : 0)
            if (value > 0)
                return value
        }
        return 0
    }

    function selectFee(feeBps) {
        root.selectedFeeBps = feeBps
        root.noteDraftChanged()
        root.quoteRequested(true)
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
        return qsTr("%1 bps").arg(feeBps)
    }

    function buildQuoteRequest() {
        var errors = []
        if (!root.hasPair) {
            errors.push(root.localIssue("token_pair_required", ["tokenAId", "tokenBId"]))
            root.localErrors = errors
            return { "ok": false, "errors": errors, "request": ({}) }
        }

        var canonicalAId = root.displayIsCanonical ? root.selectedTokenAId : root.selectedTokenBId
        var canonicalBId = root.displayIsCanonical ? root.selectedTokenBId : root.selectedTokenAId
        var request = {
            "schema": "new-position.v1",
            "tokenAId": canonicalAId,
            "tokenBId": canonicalBId,
            "feeBps": root.selectedFeeBps
        }

        if (root.activePool) {
            var parsedA = AmountMath.parseHuman(root.amountA, root.decimalsA)
            var parsedB = AmountMath.parseHuman(root.amountB, root.decimalsB)
            if (!parsedA.ok)
                errors.push(root.localIssue(parsedA.code, ["amountA"]))
            if (!parsedB.ok)
                errors.push(root.localIssue(parsedB.code, ["amountB"]))
            if (errors.length === 0) {
                request.maxAmountARaw = root.displayIsCanonical ? parsedA.raw : parsedB.raw
                request.maxAmountBRaw = root.displayIsCanonical ? parsedB.raw : parsedA.raw
                request.slippageBps = root.slippageBps
            }
        } else {
            var price = AmountMath.priceToQ64(root.initialPrice,
                                              root.canonicalDecimalsA,
                                              root.canonicalDecimalsB,
                                              root.displayIsCanonical)
            if (!price.ok)
                errors.push(root.localIssue(price.code, ["initialPrice"]))
            var scale = root.validScale()
            if (scale < 0)
                errors.push(root.localIssue("invalid_deposit_scale", ["depositScale"]))
            if (errors.length === 0) {
                request.initialPriceRealRaw = price.raw
                request.depositScaleBps = scale
            }

            if (!root.missingPool) {
                var probeA = root.probeRaw(root.tokenA, root.decimalsA)
                var probeB = root.probeRaw(root.tokenB, root.decimalsB)
                request.maxAmountARaw = root.displayIsCanonical ? probeA : probeB
                request.maxAmountBRaw = root.displayIsCanonical ? probeB : probeA
                request.slippageBps = root.slippageBps
            }
        }

        root.localErrors = errors
        return { "ok": errors.length === 0, "errors": errors, "request": request }
    }

    function probeRaw(token, decimals) {
        var balance = String(token.balanceRaw || "0")
        if (AmountMath.isUnsigned(balance) && AmountMath.compare(balance, "0") > 0)
            return balance
        return AmountMath.multiply(AmountMath.pow10(decimals), "1000")
    }

    function validScale() {
        if (!/^[0-9]+$/.test(root.depositScaleBps))
            return -1
        var scale = AmountMath.toU32(root.depositScaleBps)
        return scale >= 10000 ? scale : -1
    }

    function localIssue(code, fields) {
        return { "code": code, "blockingFields": fields, "details": ({}) }
    }

    function canonicalFieldToDisplay(field) {
        if (field === "maxAmountARaw")
            return root.displayIsCanonical ? "amountA" : "amountB"
        if (field === "maxAmountBRaw")
            return root.displayIsCanonical ? "amountB" : "amountA"
        if (field === "initialPriceRealRaw")
            return "initialPrice"
        if (field === "depositScaleBps")
            return "depositScale"
        return field
    }

    function fieldError(field) {
        var collections = [root.localErrors, root.quotePayload.errors || []]
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

    function issueText(code) {
        var messages = {
            "amount_required": qsTr("Enter a value."),
            "amount_must_be_positive": qsTr("Value must be greater than zero."),
            "invalid_amount_format": qsTr("Use plain dot-decimal format."),
            "invalid_amount_precision": qsTr("Too many decimal places for this token."),
            "invalid_raw_amount": qsTr("Value is outside the supported range."),
            "amount_exceeds_balance": qsTr("Amount exceeds the selected holding balance."),
            "amount_too_low": qsTr("Value is too low for this pool."),
            "invalid_deposit_scale": qsTr("Scale must be an integer of at least 10000 bps."),
            "amount_overflow": qsTr("Deposit scale produces an amount outside the supported range."),
            "minimum_lp_zero": qsTr("Slippage leaves no minimum LP output."),
            "invalid_slippage": qsTr("Slippage must be between 0 and 5000 bps."),
            "fee_tier_mismatch": qsTr("Select the existing pool fee tier."),
            "no_wallet": qsTr("Connect a wallet to submit this position."),
            "wallet_unavailable": qsTr("Wallet is unavailable."),
            "wallet_submission_failed": qsTr("Wallet submission failed. Review and retry manually."),
            "signature_rejected": qsTr("Wallet approval was rejected."),
            "quote_changed": qsTr("Pool or wallet state changed. Review the refreshed quote."),
            "quote_not_submittable": qsTr("Current quote cannot be submitted."),
            "submit_in_progress": qsTr("A submission is already in progress."),
            "transaction_deadline_expired": qsTr("Wallet approval expired. Retry to create a fresh request."),
            "high_slippage": qsTr("High slippage tolerance."),
            "token_definition_unreadable": qsTr("A token definition could not be read."),
            "token_program_mismatch": qsTr("Token belongs to a different TokenProgram."),
            "token_not_fungible": qsTr("Token is not a public fungible token."),
            "backend_error": qsTr("Position backend failed. Refresh and retry."),
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

        var reserveA = root.displayRaw("reserveARaw", "reserveBRaw", "A")
        var reserveB = root.displayRaw("reserveARaw", "reserveBRaw", "B")
        var parsed = AmountMath.parseHuman(value, side === "A" ? root.decimalsA : root.decimalsB)
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
        root.noteDraftChanged()
        root.quoteRequested(false)
    }

    function useMaximum() {
        var reserveA = root.displayRaw("reserveARaw", "reserveBRaw", "A")
        var reserveB = root.displayRaw("reserveARaw", "reserveBRaw", "B")
        if (!reserveA || !reserveB || reserveA === "0" || reserveB === "0")
            return
        var balanceA = String(root.tokenA.balanceRaw || "0")
        var balanceB = String(root.tokenB.balanceRaw || "0")
        var fitA = AmountMath.mulDivFloor(balanceB, reserveA, reserveB)
        var rawA = AmountMath.compare(balanceA, fitA) < 0 ? balanceA : fitA
        var rawB = AmountMath.mulDivFloor(rawA, reserveB, reserveA)
        root.amountA = AmountMath.formatRaw(rawA, root.decimalsA)
        root.amountB = AmountMath.formatRaw(rawB, root.decimalsB)
        root.noteDraftChanged()
        root.quoteRequested(true)
    }

    function editPrice(value) {
        if (root.priceTargetLp.length === 0
                && root.missingPool
                && root.quotePayload.status === "ok") {
            root.priceTargetLp = String(root.quotePayload.expectedLpRaw || "")
            root.priceAdjustmentCount = 0
        }
        root.initialPrice = value
        root.noteDraftChanged()
        root.quoteRequested(false)
    }

    function editScale(value) {
        root.depositScaleBps = value
        root.priceTargetLp = ""
        root.priceAdjustmentCount = 0
        root.noteDraftChanged()
        root.quoteRequested(false)
    }

    function stepScale(delta) {
        var current = root.validScale()
        if (current < 0)
            current = 10000
        var next = Math.max(10000, Math.min(4294967295, current + delta))
        root.depositScaleBps = String(next)
        root.priceTargetLp = ""
        root.priceAdjustmentCount = 0
        root.noteDraftChanged()
        root.quoteRequested(true)
    }

    function applyQuoteSideEffects() {
        if (root.poolFeeBps > 0 && root.selectedFeeBps !== root.poolFeeBps) {
            root.selectedFeeBps = root.poolFeeBps
            root.quoteRequested(true)
            return
        }

        if (root.quotePayload.status !== "ok")
            return

        if (root.activePool && root.amountA.length === 0 && root.amountB.length === 0) {
            root.amountA = AmountMath.formatRaw(root.displayRaw("maxAmountARaw", "maxAmountBRaw", "A"), root.decimalsA)
            root.amountB = AmountMath.formatRaw(root.displayRaw("maxAmountARaw", "maxAmountBRaw", "B"), root.decimalsB)
        }

        if (!root.missingPool || root.priceTargetLp.length === 0
                || root.priceAdjustmentCount >= 3)
            return
        var currentLp = String(root.quotePayload.expectedLpRaw || "0")
        if (currentLp === "0") {
            root.priceTargetLp = ""
            return
        }
        var desired = AmountMath.mulDivCeil(root.depositScaleBps,
                                            root.priceTargetLp,
                                            currentLp)
        if (AmountMath.compare(desired, "10000") < 0)
            desired = "10000"
        var funded = root.quotePayload.maxFundedScaleBps
        if (funded !== null && funded !== undefined && Number(funded) >= 10000
                && AmountMath.compare(desired, String(funded)) > 0) {
            desired = String(funded)
        }
        if (AmountMath.toU32(desired) >= 10000 && desired !== root.depositScaleBps) {
            root.depositScaleBps = desired
            ++root.priceAdjustmentCount
            root.quoteRequested(true)
            return
        }
        root.priceTargetLp = ""
    }

    function displayRaw(canonicalAField, canonicalBField, side) {
        if (side === "A")
            return String(root.quotePayload[root.displayIsCanonical ? canonicalAField : canonicalBField] || "")
        return String(root.quotePayload[root.displayIsCanonical ? canonicalBField : canonicalAField] || "")
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
        var amountA = root.quoteAmount("actualAmountARaw", "actualAmountBRaw", "A")
        var amountB = root.quoteAmount("actualAmountARaw", "actualAmountBRaw", "B")
        return amountA + " + " + amountB
    }

    function rawLpText(raw) {
        return raw !== undefined && raw !== null && String(raw).length > 0
                ? qsTr("%1 raw LP").arg(String(raw)) : "—"
    }

    function activePriceValue() {
        return AmountMath.priceFromQ64(String(root.quotePayload.initialPriceRealRaw || ""),
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

    function accountPreview() {
        return root.quotePayload.accountPreview || []
    }

    function quoteError() {
        if (root.quoteLoading || root.quoteStale)
            return ""
        if (root.quotePayload.status === "error")
            return root.issueText(root.quotePayload.code)
        return ""
    }

    function warningText() {
        var warnings = root.quotePayload.warnings || []
        if (warnings.length === 0)
            warnings = root.newPositionContext.warnings || []
        return warnings.length > 0 ? root.issueText(warnings[0].code) : ""
    }

    function copyToClipboard(text) {
        if (!text)
            return
        clipboardProxy.text = text
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
    }

    function submissionSnapshot() {
        var built = root.buildQuoteRequest()
        return {
            "request": built.request,
            "quoteHash": String(root.quotePayload.quoteHash || ""),
            "pairText": qsTr("%1 / %2").arg(root.shortTokenName(root.tokenA)).arg(root.shortTokenName(root.tokenB)),
            "feeText": root.feeLabel(root.selectedFeeBps),
            "depositAText": root.quoteAmount("actualAmountARaw", "actualAmountBRaw", "A"),
            "depositBText": root.quoteAmount("actualAmountARaw", "actualAmountBRaw", "B"),
            "expectedLpText": root.rawLpText(root.quotePayload.expectedLpRaw),
            "instruction": String(root.quotePayload.instruction || "")
        }
    }

    function resetAfterSubmit() {
        root.amountA = ""
        root.amountB = ""
        root.initialPrice = ""
        root.depositScaleBps = "10000"
        root.localErrors = []
        root.submitError = ""
        root.priceTargetLp = ""
        root.priceAdjustmentCount = 0
    }

    function tokenLabel(token) {
        if (!token || !token.definitionId)
            return qsTr("Select token")
        var name = token.name && token.name.length > 0 ? token.name : qsTr("Unknown token")
        return qsTr("%1 · %2").arg(name).arg(root.shortId(token.definitionId))
    }

    function tokenDetail(token) {
        if (!token || !token.definitionId)
            return ""
        return qsTr("%1\n%2 implied decimals\nBalance %3")
                .arg(token.definitionId)
                .arg(AmountMath.implyDecimals(token.totalSupplyRaw || "0"))
                .arg(root.balanceText(token, AmountMath.implyDecimals(token.totalSupplyRaw || "0")))
    }

    function shortTokenName(token) {
        if (token && token.name && token.name.length > 0)
            return token.name
        return token && token.definitionId ? root.shortId(token.definitionId) : "—"
    }

    function balanceText(token, decimals) {
        return AmountMath.formatRaw(String(token.balanceRaw || "0"), decimals)
    }

    function shortId(value) {
        var text = String(value || "")
        return text.length > 14 ? text.slice(0, 7) + "…" + text.slice(-5) : text
    }

    function scaleHelpText() {
        var error = root.fieldError("depositScale")
        if (error.length > 0)
            return error
        var funded = root.quotePayload.maxFundedScaleBps
        if (funded === null || funded === undefined)
            return ""
        if (Number(funded) === 0)
            return qsTr("Current holdings cannot fund the minimum deposit. Simulation remains available.")
        return qsTr("Current holdings fund up to %1 bps.").arg(funded)
    }

    function contextStatusText() {
        var network = String(root.newPositionContext.networkId || "")
        if (root.newPositionContext.status === "no_wallet")
            return qsTr("%1 · simulation only").arg(network || qsTr("Wallet disconnected"))
        if (root.newPositionContext.status === "ready")
            return qsTr("%1 · wallet ready").arg(network)
        return network.length > 0 ? network : qsTr("Loading network")
    }

    function contextBlocksForm() {
        var status = String(root.newPositionContext.status || "")
        return status !== "" && status !== "ready" && status !== "no_wallet" && status !== "loading"
    }

    function contextErrorText() {
        return root.issueText(root.newPositionContext.code || root.newPositionContext.status)
    }
}
