import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../shared"
import "../../state"

Rectangle {
    id: root

    required property NewPositionPrototypeBackend prototypeBackend

    property int selectedTokenAIndex: 0
    property int selectedTokenBIndex: 1
    property int selectedFeeBps: 30
    property real slippageTolerancePercent: 0.5
    property string amountA: ""
    property string amountB: ""
    property string editedSide: "A"
    property string initialPrice: "2500"
    property int depositScale: 1
    property string submitError: ""

    readonly property var emptyToken: ({
            "symbol": "",
            "balance": 0,
            "balanceText": "",
            "accent": "#343434"
        })
    readonly property var holdings: root.prototypeBackend ? root.prototypeBackend.holdings : []
    readonly property var tokenA: root.holdings[root.selectedTokenAIndex] || root.emptyToken
    readonly property var tokenB: root.holdings[root.selectedTokenBIndex] || root.emptyToken
    readonly property var poolContext: root.prototypeBackend.poolContext(root.tokenA.symbol, root.tokenB.symbol)
    readonly property string poolStatus: root.poolContext.poolStatus
    readonly property bool activePool: root.poolStatus === "active_pool"
    readonly property bool missingPool: root.poolStatus === "missing_pool"
    readonly property int slippageBps: Math.round(root.slippageTolerancePercent * 100)
    readonly property var quote: root.prototypeBackend.quoteNewPosition(root.buildRequest())
    readonly property bool canConfirm: root.quote.status === "ok"
    readonly property bool compact: root.width < 820
    readonly property bool hasActiveInput: root.amountA.length > 0 || root.amountB.length > 0
    readonly property bool showQuoteError: root.quote.status === "error" && (root.poolStatus === "unavailable_pool" || root.missingPool || root.hasActiveInput)

    signal confirmationRequested(var snapshot)

    color: "#1B1B1B"
    implicitHeight: content.implicitHeight + 24
    radius: 12
    border.color: "#303030"
    border.width: 1

    Component.onCompleted: root.afterPairChanged()

    onSelectedTokenAIndexChanged: root.afterPairChanged()
    onSelectedTokenBIndexChanged: root.afterPairChanged()
    onPoolStatusChanged: root.normalizeFeeSelection()

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: 12
        spacing: 12

        RowLayout {
            spacing: 12

            Layout.fillWidth: true

            ColumnLayout {
                spacing: 2

                Layout.fillWidth: true

                Text {
                    color: "#E7E1D8"
                    font.bold: true
                    font.pixelSize: 18
                    text: qsTr("New position")

                    Layout.fillWidth: true
                }

                Text {
                    color: "#A9A098"
                    elide: Text.ElideRight
                    font.pixelSize: 12
                    text: qsTr("Active account %1").arg(root.prototypeBackend.activeAccount)

                    Layout.fillWidth: true
                }
            }

            Rectangle {
                color: "#211914"
                radius: 12
                border.color: "#49301F"
                border.width: 1

                Layout.preferredHeight: 28
                Layout.preferredWidth: pairText.implicitWidth + 20

                Text {
                    id: pairText

                    anchors.centerIn: parent
                    color: "#F2D8C7"
                    font.bold: true
                    font.pixelSize: 12
                    text: qsTr("%1 / %2").arg(root.tokenA.symbol).arg(root.tokenB.symbol)
                }
            }
        }

        Rectangle {
            color: root.statusBackgroundColor()
            radius: 8
            border.color: root.statusBorderColor()
            border.width: 1

            Layout.fillWidth: true
            Layout.preferredHeight: statusRow.implicitHeight + 18

            RowLayout {
                id: statusRow

                anchors.fill: parent
                anchors.margins: 10
                spacing: 10

                Rectangle {
                    color: root.statusColor()
                    radius: 5

                    Layout.preferredHeight: 10
                    Layout.preferredWidth: 10
                }

                ColumnLayout {
                    spacing: 2

                    Layout.fillWidth: true

                    Text {
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 13
                        text: root.quote.statusLabel

                        Layout.fillWidth: true
                    }

                    Text {
                        color: "#A9A098"
                        font.pixelSize: 12
                        lineHeight: 1.2
                        text: root.quote.statusDetail
                        wrapMode: Text.WordWrap

                        Layout.fillWidth: true
                    }
                }
            }
        }

        GridLayout {
            columns: 1
            columnSpacing: 12
            rowSpacing: 12

            Layout.fillWidth: true

            Rectangle {
                color: "#151515"
                radius: 8
                border.color: "#303030"
                border.width: 1

                Layout.fillWidth: true
                Layout.preferredHeight: selectionContent.implicitHeight + 20

                ColumnLayout {
                    id: selectionContent

                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 12

                    Text {
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 14
                        text: qsTr("Pair")

                        Layout.fillWidth: true
                    }

                    GridLayout {
                        columns: root.compact ? 1 : 2
                        columnSpacing: 8
                        rowSpacing: 8

                        Layout.fillWidth: true

                        ColumnLayout {
                            spacing: 6

                            Layout.fillWidth: true

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Token A")

                                Layout.fillWidth: true
                            }

                            Repeater {
                                model: root.prototypeBackend.holdings

                                delegate: Button {
                                    id: tokenAButton

                                    readonly property var holding: modelData
                                    readonly property bool selected: root.selectedTokenAIndex === index
                                    readonly property bool duplicate: root.selectedTokenBIndex === index

                                    activeFocusOnTab: true
                                    enabled: !duplicate
                                    focusPolicy: Qt.StrongFocus
                                    hoverEnabled: true
                                    text: holding.symbol

                                    Accessible.name: qsTr("Select %1 as token A").arg(holding.symbol)

                                    Layout.fillWidth: true
                                    Layout.minimumHeight: 48

                                    onClicked: root.chooseToken("A", index)

                                    contentItem: RowLayout {
                                        spacing: 8

                                        Rectangle {
                                            color: tokenAButton.holding.accent
                                            radius: 5

                                            Layout.preferredHeight: 10
                                            Layout.preferredWidth: 10
                                        }

                                        ColumnLayout {
                                            spacing: 1

                                            Layout.fillWidth: true

                                            Text {
                                                color: tokenAButton.enabled ? "#E7E1D8" : "#6E6862"
                                                elide: Text.ElideRight
                                                font.bold: true
                                                font.pixelSize: 12
                                                text: tokenAButton.holding.symbol

                                                Layout.fillWidth: true
                                            }

                                            Text {
                                                color: tokenAButton.enabled ? "#A9A098" : "#5D5751"
                                                elide: Text.ElideRight
                                                font.pixelSize: 10
                                                text: tokenAButton.holding.balanceText

                                                Layout.fillWidth: true
                                            }
                                        }
                                    }

                                    background: Rectangle {
                                        border.color: tokenAButton.activeFocus || tokenAButton.selected ? "#F26A21" : "#343434"
                                        border.width: 1
                                        color: tokenAButton.selected ? "#211914" : tokenAButton.hovered || tokenAButton.activeFocus ? "#202020" : "#101010"
                                        radius: 6
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            spacing: 6

                            Layout.fillWidth: true

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Token B")

                                Layout.fillWidth: true
                            }

                            Repeater {
                                model: root.prototypeBackend.holdings

                                delegate: Button {
                                    id: tokenBButton

                                    readonly property var holding: modelData
                                    readonly property bool selected: root.selectedTokenBIndex === index
                                    readonly property bool duplicate: root.selectedTokenAIndex === index

                                    activeFocusOnTab: true
                                    enabled: !duplicate
                                    focusPolicy: Qt.StrongFocus
                                    hoverEnabled: true
                                    text: holding.symbol

                                    Accessible.name: qsTr("Select %1 as token B").arg(holding.symbol)

                                    Layout.fillWidth: true
                                    Layout.minimumHeight: 48

                                    onClicked: root.chooseToken("B", index)

                                    contentItem: RowLayout {
                                        spacing: 8

                                        Rectangle {
                                            color: tokenBButton.holding.accent
                                            radius: 5

                                            Layout.preferredHeight: 10
                                            Layout.preferredWidth: 10
                                        }

                                        ColumnLayout {
                                            spacing: 1

                                            Layout.fillWidth: true

                                            Text {
                                                color: tokenBButton.enabled ? "#E7E1D8" : "#6E6862"
                                                elide: Text.ElideRight
                                                font.bold: true
                                                font.pixelSize: 12
                                                text: tokenBButton.holding.symbol

                                                Layout.fillWidth: true
                                            }

                                            Text {
                                                color: tokenBButton.enabled ? "#A9A098" : "#5D5751"
                                                elide: Text.ElideRight
                                                font.pixelSize: 10
                                                text: tokenBButton.holding.balanceText

                                                Layout.fillWidth: true
                                            }
                                        }
                                    }

                                    background: Rectangle {
                                        border.color: tokenBButton.activeFocus || tokenBButton.selected ? "#F26A21" : "#343434"
                                        border.width: 1
                                        color: tokenBButton.selected ? "#211914" : tokenBButton.hovered || tokenBButton.activeFocus ? "#202020" : "#101010"
                                        radius: 6
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 14
                        text: qsTr("Fee tier")

                        Layout.fillWidth: true
                    }

                    GridLayout {
                        columns: root.compact ? 2 : 3
                        columnSpacing: 6
                        rowSpacing: 6

                        Layout.fillWidth: true

                        Repeater {
                            model: root.prototypeBackend.feeTiers

                            delegate: Rectangle {
                                id: feeChip

                                readonly property var tier: modelData
                                readonly property string disabledReason: root.feeTierDisabledReason(tier)
                                readonly property bool chipEnabled: disabledReason.length === 0
                                readonly property bool selected: root.selectedFeeBps === tier.bps

                                activeFocusOnTab: chipEnabled
                                color: selected ? "#211914" : feeMouse.containsMouse && chipEnabled ? "#202020" : "#101010"
                                radius: 6
                                border.color: selected || activeFocus ? "#F26A21" : chipEnabled ? "#343434" : "#2A2A2A"
                                border.width: 1
                                opacity: chipEnabled ? 1 : 0.58

                                Accessible.name: qsTr("Fee tier %1").arg(tier.label)
                                Accessible.role: Accessible.Button
                                Accessible.description: disabledReason

                                Layout.fillWidth: true
                                Layout.minimumHeight: 42

                                Keys.onSpacePressed: {
                                    if (chipEnabled)
                                        root.selectFeeTier(tier.bps);
                                }
                                Keys.onReturnPressed: {
                                    if (chipEnabled)
                                        root.selectFeeTier(tier.bps);
                                }

                                Text {
                                    anchors.centerIn: parent
                                    color: feeChip.selected ? "#F2D8C7" : feeChip.chipEnabled ? "#E7E1D8" : "#7D756E"
                                    font.bold: true
                                    font.pixelSize: 12
                                    text: feeChip.tier.label
                                }

                                MouseArea {
                                    id: feeMouse

                                    anchors.fill: parent
                                    cursorShape: feeChip.chipEnabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                                    hoverEnabled: true

                                    onClicked: {
                                        if (feeChip.chipEnabled) {
                                            feeChip.forceActiveFocus();
                                            root.selectFeeTier(feeChip.tier.bps);
                                        }
                                    }
                                }

                                ToolTip.delay: 300
                                ToolTip.text: disabledReason
                                ToolTip.visible: feeMouse.containsMouse && disabledReason.length > 0
                            }
                        }
                    }

                    SummaryRow {
                        label: qsTr("Pool id")
                        value: root.quote.pool.id.length > 0 ? root.quote.pool.id : "-"

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Pool price")
                        value: root.quote.pool.priceText

                        Layout.fillWidth: true
                    }

                    SummaryRow {
                        label: qsTr("Reserves")
                        value: root.quote.pool.reserveText

                        Layout.fillWidth: true
                    }
                }
            }

            Rectangle {
                color: "#151515"
                radius: 8
                border.color: "#303030"
                border.width: 1

                Layout.fillWidth: true
                Layout.preferredHeight: depositContent.implicitHeight + 20

                ColumnLayout {
                    id: depositContent

                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 12

                    Text {
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 14
                        text: root.activePool ? qsTr("Ratio-locked deposit") : root.missingPool ? qsTr("Initial price deposit") : qsTr("Deposit unavailable")

                        Layout.fillWidth: true
                    }

                    ColumnLayout {
                        spacing: 10
                        visible: root.activePool

                        Layout.fillWidth: true
                        Layout.preferredHeight: visible ? implicitHeight : 0

                        TokenAmountInput {
                            balance: root.tokenA.balanceText
                            errorText: root.showQuoteError && root.editedSide === "A" ? root.quote.error : ""
                            helperText: root.editedSide === "B" && root.amountA.length > 0 ? qsTr("Locked by pool ratio") : ""
                            label: qsTr("%1 deposit").arg(root.tokenA.symbol)
                            token: root.tokenA.symbol
                            text: root.amountA

                            Layout.fillWidth: true

                            onEditingChanged: function (value) {
                                root.editActiveAmount("A", value);
                            }
                            onMaxClicked: root.useMaxActive("A")
                        }

                        TokenAmountInput {
                            balance: root.tokenB.balanceText
                            errorText: root.showQuoteError && root.editedSide === "B" ? root.quote.error : ""
                            helperText: root.editedSide === "A" && root.amountB.length > 0 ? qsTr("Locked by pool ratio") : ""
                            label: qsTr("%1 deposit").arg(root.tokenB.symbol)
                            token: root.tokenB.symbol
                            text: root.amountB

                            Layout.fillWidth: true

                            onEditingChanged: function (value) {
                                root.editActiveAmount("B", value);
                            }
                            onMaxClicked: root.useMaxActive("B")
                        }
                    }

                    ColumnLayout {
                        spacing: 10
                        visible: root.missingPool

                        Layout.fillWidth: true
                        Layout.preferredHeight: visible ? implicitHeight : 0

                        Text {
                            color: "#A9A098"
                            font.pixelSize: 12
                            lineHeight: 1.2
                            text: qsTr("Price is locked before deposit sizing. Scaling preserves the same initial price.")
                            wrapMode: Text.WordWrap

                            Layout.fillWidth: true
                        }

                        Rectangle {
                            color: priceField.activeFocus ? "#1F1B18" : "#101010"
                            radius: 6
                            border.color: priceField.activeFocus ? "#F26A21" : "#343434"
                            border.width: 1

                            Layout.fillWidth: true
                            Layout.minimumHeight: 64

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 4

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Initial price: 1 %1 in %2").arg(root.tokenB.symbol).arg(root.tokenA.symbol)

                                    Layout.fillWidth: true
                                }

                                TextField {
                                    id: priceField

                                    activeFocusOnTab: true
                                    color: "#E7E1D8"
                                    font.bold: true
                                    font.pixelSize: 18
                                    inputMethodHints: Qt.ImhFormattedNumbersOnly
                                    placeholderText: qsTr("0")
                                    selectByMouse: true
                                    selectedTextColor: "#151515"
                                    selectionColor: "#F26A21"
                                    text: root.initialPrice
                                    validator: RegularExpressionValidator {
                                        regularExpression: /[0-9]*([.][0-9]*)?/
                                    }

                                    Accessible.name: qsTr("Initial pool price")

                                    Layout.fillWidth: true

                                    onTextEdited: {
                                        root.submitError = "";
                                        root.initialPrice = text;
                                    }

                                    background: Item {}
                                }
                            }
                        }

                        RowLayout {
                            spacing: 6

                            Layout.fillWidth: true

                            Repeater {
                                model: [1, 2, 5]

                                delegate: Button {
                                    id: scaleButton

                                    readonly property int scaleValue: modelData
                                    readonly property bool selected: root.depositScale === scaleValue

                                    activeFocusOnTab: true
                                    focusPolicy: Qt.StrongFocus
                                    hoverEnabled: true
                                    text: qsTr("%1x").arg(scaleValue)

                                    Accessible.name: qsTr("Scale deposit to %1 times minimum").arg(scaleValue)

                                    Layout.fillWidth: true
                                    Layout.minimumHeight: 42

                                    onClicked: {
                                        root.submitError = "";
                                        root.depositScale = scaleValue;
                                    }

                                    contentItem: Text {
                                        color: scaleButton.selected ? "#F2D8C7" : scaleButton.hovered || scaleButton.activeFocus ? "#E7E1D8" : "#A9A098"
                                        font.bold: true
                                        font.pixelSize: 12
                                        horizontalAlignment: Text.AlignHCenter
                                        text: scaleButton.text
                                        verticalAlignment: Text.AlignVCenter
                                    }

                                    background: Rectangle {
                                        border.color: scaleButton.activeFocus || scaleButton.selected ? "#F26A21" : "#343434"
                                        border.width: 1
                                        color: scaleButton.selected ? "#211914" : scaleButton.hovered || scaleButton.activeFocus ? "#202020" : "#101010"
                                        radius: 6
                                    }
                                }
                            }
                        }

                        TokenAmountInput {
                            balance: root.tokenA.balanceText
                            helperText: qsTr("Minimum deposit at locked price")
                            label: qsTr("%1 deposit").arg(root.tokenA.symbol)
                            readOnly: true
                            showMaxButton: false
                            token: root.tokenA.symbol
                            text: root.quote.deposit.maxA.input

                            Layout.fillWidth: true
                        }

                        TokenAmountInput {
                            balance: root.tokenB.balanceText
                            helperText: qsTr("Scaled with %1").arg(root.tokenA.symbol)
                            label: qsTr("%1 deposit").arg(root.tokenB.symbol)
                            readOnly: true
                            showMaxButton: false
                            token: root.tokenB.symbol
                            text: root.quote.deposit.maxB.input

                            Layout.fillWidth: true
                        }
                    }

                    Rectangle {
                        color: "#211914"
                        radius: 8
                        border.color: "#49301F"
                        border.width: 1
                        visible: root.showQuoteError || root.submitError.length > 0

                        Layout.fillWidth: true
                        Layout.preferredHeight: visible ? errorText.implicitHeight + 20 : 0

                        Text {
                            id: errorText

                            anchors.fill: parent
                            anchors.margins: 10
                            color: "#F08A76"
                            font.pixelSize: 12
                            lineHeight: 1.2
                            text: root.submitError.length > 0 ? root.submitError : root.quote.error
                            wrapMode: Text.WordWrap
                        }
                    }

                    SlippageToleranceControl {
                        tolerancePercent: root.slippageTolerancePercent
                        visible: root.activePool || root.missingPool

                        Layout.fillWidth: true
                        Layout.preferredHeight: visible ? implicitHeight : 0

                        onToleranceChangeRequested: function (tolerancePercent) {
                            root.submitError = "";
                            root.slippageTolerancePercent = Math.max(0.01, Math.min(50, Number(tolerancePercent) || 0));
                        }
                    }

                    Button {
                        id: confirmButton

                        activeFocusOnTab: true
                        enabled: root.canConfirm
                        focusPolicy: Qt.StrongFocus
                        hoverEnabled: true
                        text: root.canConfirm ? qsTr("Preview and confirm") : qsTr("Preview unavailable")

                        Accessible.name: confirmButton.text

                        Layout.fillWidth: true
                        Layout.minimumHeight: 44

                        onClicked: root.confirmationRequested(root.submitSnapshot())

                        contentItem: Text {
                            color: confirmButton.enabled ? "#151515" : "#7D756E"
                            elide: Text.ElideRight
                            font.bold: true
                            font.pixelSize: 13
                            horizontalAlignment: Text.AlignHCenter
                            text: confirmButton.text
                            verticalAlignment: Text.AlignVCenter
                        }

                        background: Rectangle {
                            border.color: confirmButton.enabled ? "#F26A21" : "#343434"
                            border.width: 1
                            color: confirmButton.enabled ? confirmButton.pressed ? "#D95C1E" : confirmButton.hovered || confirmButton.activeFocus ? "#FF8A3D" : "#F26A21" : "#181818"
                            radius: 6
                        }
                    }
                }
            }
        }

        Rectangle {
            color: "#151515"
            radius: 8
            border.color: "#303030"
            border.width: 1

            Layout.fillWidth: true
            Layout.preferredHeight: previewContent.implicitHeight + 20

            ColumnLayout {
                id: previewContent

                anchors.fill: parent
                anchors.margins: 10
                spacing: 10

                RowLayout {
                    spacing: 8

                    Layout.fillWidth: true

                    Text {
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 14
                        text: qsTr("Preview")

                        Layout.fillWidth: true
                    }

                    Text {
                        color: root.quote.status === "ok" ? "#8FD6A4" : "#F08A76"
                        font.bold: true
                        font.pixelSize: 11
                        horizontalAlignment: Text.AlignRight
                        text: root.quote.status === "ok" ? qsTr("Ready") : qsTr("Needs input")

                        Layout.maximumWidth: 120
                    }
                }

                GridLayout {
                    columns: root.compact ? 1 : 2
                    columnSpacing: 16
                    rowSpacing: 8

                    Layout.fillWidth: true

                    ColumnLayout {
                        spacing: 8

                        Layout.fillWidth: true

                        SummaryRow {
                            label: qsTr("Instruction")
                            value: root.instructionText()

                            Layout.fillWidth: true
                        }

                        SummaryRow {
                            label: qsTr("Deposit %1").arg(root.tokenA.symbol)
                            value: root.quote.deposit.actualA.display

                            Layout.fillWidth: true
                        }

                        SummaryRow {
                            label: qsTr("Deposit %1").arg(root.tokenB.symbol)
                            value: root.quote.deposit.actualB.display

                            Layout.fillWidth: true
                        }

                        SummaryRow {
                            label: qsTr("Expected LP")
                            value: root.quote.lp.expected.display
                            estimated: true
                            estimateHelp: qsTr("Prototype quote mirrors the future backend response shape.")

                            Layout.fillWidth: true
                        }
                    }

                    ColumnLayout {
                        spacing: 8

                        Layout.fillWidth: true

                        SummaryRow {
                            label: qsTr("Minimum LP")
                            value: root.quote.lp.minimum.display

                            Layout.fillWidth: true
                        }

                        SummaryRow {
                            label: qsTr("Locked LP")
                            value: root.quote.lp.locked.display
                            visible: root.missingPool

                            Layout.fillWidth: true
                        }

                        SummaryRow {
                            label: qsTr("Current position")
                            value: root.quote.position.userLp

                            Layout.fillWidth: true
                        }

                        SummaryRow {
                            label: qsTr("Pool share")
                            value: root.quote.position.share

                            Layout.fillWidth: true
                        }
                    }
                }

                Rectangle {
                    color: "#101010"
                    radius: 8
                    border.color: "#303030"
                    border.width: 1
                    visible: root.quote.accountChanges.length > 0

                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? accountList.implicitHeight + 16 : 0

                    ColumnLayout {
                        id: accountList

                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 6

                        Text {
                            color: "#A9A098"
                            font.pixelSize: 12
                            text: qsTr("Account changes")

                            Layout.fillWidth: true
                        }

                        Repeater {
                            model: root.quote.accountChanges

                            delegate: Rectangle {
                                id: accountRow

                                readonly property var accountChange: modelData

                                color: "#151515"
                                radius: 6
                                border.color: "#2C2C2C"
                                border.width: 1

                                Layout.fillWidth: true
                                Layout.preferredHeight: rowLayout.implicitHeight + 12

                                RowLayout {
                                    id: rowLayout

                                    anchors.fill: parent
                                    anchors.leftMargin: 8
                                    anchors.rightMargin: 8
                                    spacing: 8

                                    Rectangle {
                                        color: root.accountActionColor(accountRow.accountChange.action)
                                        radius: 4

                                        Layout.preferredHeight: 8
                                        Layout.preferredWidth: 8
                                    }

                                    Text {
                                        color: "#E7E1D8"
                                        elide: Text.ElideRight
                                        font.bold: true
                                        font.pixelSize: 12
                                        text: accountRow.accountChange.role

                                        Layout.preferredWidth: 120
                                    }

                                    Text {
                                        color: "#A9A098"
                                        elide: Text.ElideMiddle
                                        font.pixelSize: 11
                                        text: accountRow.accountChange.id

                                        Layout.fillWidth: true
                                    }

                                    Text {
                                        color: "#F2D8C7"
                                        elide: Text.ElideRight
                                        font.bold: true
                                        font.pixelSize: 11
                                        horizontalAlignment: Text.AlignRight
                                        text: accountRow.accountChange.action

                                        Layout.preferredWidth: 100
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    function buildRequest() {
        return {
            "amountA": root.amountA,
            "amountB": root.amountB,
            "depositScale": root.depositScale,
            "editedSide": root.editedSide,
            "feeBps": root.selectedFeeBps,
            "initialPrice": root.initialPrice,
            "slippageBps": root.slippageBps,
            "tokenA": root.tokenA.symbol,
            "tokenB": root.tokenB.symbol
        };
    }

    function chooseToken(side, index) {
        root.submitError = "";

        if (side === "A") {
            root.selectedTokenAIndex = index;
            if (root.selectedTokenBIndex === index)
                root.selectedTokenBIndex = root.nextTokenIndex(index);
            return;
        }

        root.selectedTokenBIndex = index;
        if (root.selectedTokenAIndex === index)
            root.selectedTokenAIndex = root.nextTokenIndex(index);
    }

    function nextTokenIndex(index) {
        return root.holdings.length > 0 ? (index + 1) % root.holdings.length : 0;
    }

    function afterPairChanged() {
        if (root.selectedTokenAIndex === root.selectedTokenBIndex)
            root.selectedTokenBIndex = root.nextTokenIndex(root.selectedTokenAIndex);

        if (root.tokenA.symbol.length === 0 || root.tokenB.symbol.length === 0)
            return;

        root.submitError = "";
        root.amountA = "";
        root.amountB = "";
        root.editedSide = "A";
        root.initialPrice = root.prototypeBackend.formatAmount(root.prototypeBackend.defaultInitialPrice(root.tokenA.symbol, root.tokenB.symbol));
        root.depositScale = 1;
        root.normalizeFeeSelection();
    }

    function normalizeFeeSelection() {
        if (root.tokenA.symbol.length === 0 || root.tokenB.symbol.length === 0)
            return;

        const context = root.prototypeBackend.poolContext(root.tokenA.symbol, root.tokenB.symbol);

        if (context.poolStatus === "active_pool") {
            root.selectedFeeBps = context.storedFeeBps;
            return;
        }

        if (root.selectedFeeBps === 25)
            root.selectedFeeBps = 30;
    }

    function selectFeeTier(bps) {
        root.submitError = "";
        root.selectedFeeBps = bps;
    }

    function feeTierDisabledReason(tier) {
        if (!tier.supported)
            return qsTr("Unsupported by the AMM program. Valid fee tiers are 0.01%, 0.05%, 0.30%, and 1.00%.");

        if (root.poolStatus === "active_pool" && tier.bps !== root.poolContext.storedFeeBps)
            return qsTr("Existing pool uses %1. Fee tier is fixed by pool configuration.").arg(root.prototypeBackend.feeLabel(root.poolContext.storedFeeBps));

        if (root.poolStatus === "unavailable_pool")
            return qsTr("Fee selection is disabled until this pair can be quoted.");

        return "";
    }

    function editActiveAmount(side, value) {
        root.submitError = "";
        root.editedSide = side;

        if (side === "A")
            root.amountA = value;
        else
            root.amountB = value;

        root.syncActiveCounterpart();
    }

    function syncActiveCounterpart() {
        const nextQuote = root.prototypeBackend.quoteNewPosition(root.buildRequest());

        if (nextQuote.status !== "ok" || nextQuote.poolStatus !== "active_pool")
            return;

        if (root.editedSide === "A")
            root.amountB = nextQuote.deposit.maxB.input;
        else
            root.amountA = nextQuote.deposit.maxA.input;
    }

    function useMaxActive(side) {
        const ratio = root.prototypeBackend.activeRatio(root.tokenA.symbol, root.tokenB.symbol);
        const maxA = Math.min(root.tokenA.balance, root.tokenB.balance / ratio);
        const maxB = Math.min(root.tokenB.balance, root.tokenA.balance * ratio);

        if (side === "A")
            root.editActiveAmount("A", root.prototypeBackend.formatAmount(maxA));
        else
            root.editActiveAmount("B", root.prototypeBackend.formatAmount(maxB));
    }

    function setSubmitError(message) {
        root.submitError = message;
    }

    function resetAfterSubmit() {
        root.submitError = "";

        if (root.activePool) {
            root.amountA = "";
            root.amountB = "";
            root.editedSide = "A";
        }
    }

    function submitSnapshot() {
        return {
            "depositA": root.quote.deposit.actualA.display,
            "depositB": root.quote.deposit.actualB.display,
            "expectedLp": root.quote.lp.expected.display,
            "feeLabel": root.quote.feeLabel,
            "instructionText": root.instructionText(),
            "minimumLp": root.quote.lp.minimum.display,
            "pairText": qsTr("%1 / %2").arg(root.tokenA.symbol).arg(root.tokenB.symbol),
            "quote": root.quote,
            "quoteHash": root.quote.quoteHash,
            "request": root.buildRequest(),
            "shortQuoteHash": root.quote.quoteHash.substring(0, 18),
            "tokenA": root.tokenA.symbol,
            "tokenB": root.tokenB.symbol
        };
    }

    function instructionText() {
        if (root.quote.instruction === "new_definition")
            return qsTr("Create pool");

        if (root.quote.instruction === "add_liquidity")
            return qsTr("Add liquidity");

        return qsTr("Unavailable");
    }

    function statusColor() {
        if (root.poolStatus === "active_pool")
            return "#8FD6A4";

        if (root.poolStatus === "missing_pool")
            return "#F2B366";

        return "#F08A76";
    }

    function statusBackgroundColor() {
        if (root.poolStatus === "active_pool")
            return "#162218";

        if (root.poolStatus === "missing_pool")
            return "#211914";

        return "#241817";
    }

    function statusBorderColor() {
        if (root.poolStatus === "active_pool")
            return "#2E5A39";

        if (root.poolStatus === "missing_pool")
            return "#49301F";

        return "#5A3028";
    }

    function accountActionColor(action) {
        if (action === qsTr("Create"))
            return "#F2B366";

        if (action === qsTr("Read"))
            return "#8BA8E8";

        return "#8FD6A4";
    }
}
