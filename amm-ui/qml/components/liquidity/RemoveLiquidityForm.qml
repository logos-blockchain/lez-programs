import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../shared"
import "../../state"

Rectangle {
    id: root

    required property DummyPoolState poolState

    property real slippageTolerancePercent: 0.5
    property int burnAmount: 0
    readonly property int maxBurnAmount: root.poolState.clampBurnAmount(root.poolState.userLpBalance)
    readonly property bool hasLpTokens: root.maxBurnAmount > 0
    readonly property int preset25Amount: root.poolState.burnAmountForPercent(25)
    readonly property int preset50Amount: root.poolState.burnAmountForPercent(50)
    readonly property int preset75Amount: root.poolState.burnAmountForPercent(75)
    readonly property real removePercent: root.maxBurnAmount > 0 ? root.burnAmount * 100 / root.maxBurnAmount : 0
    readonly property var preview: root.poolState.removeLiquidityPreview(root.burnAmount)
    readonly property int minTokenAReceived: root.poolState.minReceivedAmount(root.preview.withdrawA, root.slippageTolerancePercent)
    readonly property int minTokenBReceived: root.poolState.minReceivedAmount(root.preview.withdrawB, root.slippageTolerancePercent)
    readonly property bool minReceivedIsZero: root.burnAmount > 0 && (root.minTokenAReceived === 0 || root.minTokenBReceived === 0)
    readonly property bool canSubmit: root.hasLpTokens && root.burnAmount > 0 && !root.minReceivedIsZero
    readonly property string estimateHelp: qsTr("Estimated with the same integer floor math used by the remove-liquidity contract path.")
    readonly property string submitButtonText: !root.hasLpTokens ? qsTr("No LP balance") : root.burnAmount === 0 ? qsTr("Enter an amount") : root.minReceivedIsZero ? qsTr("Minimum received is 0") : qsTr("Remove Liquidity")

    signal slippageToleranceChangeRequested(real tolerancePercent)
    signal removeLiquidityRequested(var snapshot)

    color: "#00000000"
    implicitHeight: content.implicitHeight
    radius: 0
    border.width: 0

    onMaxBurnAmountChanged: {
        if (root.burnAmount > root.maxBurnAmount) {
            root.setBurnAmount(root.maxBurnAmount);
        }
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        spacing: 10

        Text {
            color: "#F26A21"
            font.pixelSize: 12
            text: qsTr("No LP tokens")
            visible: !root.hasLpTokens

            Layout.fillWidth: true
        }

        Text {
            color: "#A9A098"
            font.pixelSize: 12
            lineHeight: 1.25
            text: qsTr("Add liquidity first to receive LP tokens before removing from this pool.")
            visible: !root.hasLpTokens
            wrapMode: Text.WordWrap

            Layout.fillWidth: true
        }

        Rectangle {
            color: root.hasLpTokens ? "#151515" : "#121212"
            radius: 8
            border.color: burnField.activeFocus ? "#F26A21" : "#343434"
            border.width: 1

            Layout.fillWidth: true
            Layout.preferredHeight: inputContent.implicitHeight + 20

            ColumnLayout {
                id: inputContent

                anchors.fill: parent
                anchors.margins: 10
                spacing: 8

                RowLayout {
                    spacing: 8

                    Layout.fillWidth: true

                    Text {
                        color: "#A9A098"
                        elide: Text.ElideRight
                        font.pixelSize: 12
                        text: qsTr("LP tokens to burn")

                        Layout.fillWidth: true
                    }

                    Text {
                        color: "#A9A098"
                        elide: Text.ElideRight
                        font.pixelSize: 11
                        horizontalAlignment: Text.AlignRight
                        text: qsTr("Available LP: %1").arg(root.poolState.formatInteger(root.poolState.userLpBalance))

                        Layout.maximumWidth: 170
                    }
                }

                TextField {
                    id: burnField

                    activeFocusOnTab: root.hasLpTokens
                    color: "#E7E1D8"
                    enabled: root.hasLpTokens
                    font.bold: true
                    font.pixelSize: 18
                    inputMethodHints: Qt.ImhDigitsOnly
                    placeholderText: qsTr("0")
                    selectByMouse: true
                    selectedTextColor: "#151515"
                    selectionColor: "#F26A21"
                    text: root.burnAmount > 0 ? String(root.burnAmount) : ""
                    validator: RegularExpressionValidator {
                        regularExpression: /[0-9]*/
                    }

                    Accessible.name: qsTr("LP tokens to burn")

                    Layout.fillWidth: true
                    Layout.minimumHeight: 44

                    onTextEdited: root.setBurnAmount(text)

                    background: Rectangle {
                        border.color: burnField.activeFocus ? "#F26A21" : "#343434"
                        border.width: 1
                        color: burnField.activeFocus ? "#1F1B18" : "#101010"
                        radius: 6
                    }
                }
            }
        }

        RowLayout {
            spacing: 6

            Layout.fillWidth: true

            Button {
                id: preset25

                activeFocusOnTab: root.hasLpTokens
                enabled: root.hasLpTokens
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("25%")

                Accessible.name: qsTr("Remove 25 percent")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.setBurnPercent(25)

                contentItem: Text {
                    color: preset25.enabled && (preset25.hovered || preset25.activeFocus || root.preset25Amount === root.burnAmount) ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset25.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset25.activeFocus || root.preset25Amount === root.burnAmount ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset25.pressed ? "#D95C1E" : root.preset25Amount === root.burnAmount ? "#F26A21" : preset25.hovered || preset25.activeFocus ? "#E7E1D8" : "#151515"
                    radius: 6
                }
            }

            Button {
                id: preset50

                activeFocusOnTab: root.hasLpTokens
                enabled: root.hasLpTokens
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("50%")

                Accessible.name: qsTr("Remove 50 percent")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.setBurnPercent(50)

                contentItem: Text {
                    color: preset50.enabled && (preset50.hovered || preset50.activeFocus || root.preset50Amount === root.burnAmount) ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset50.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset50.activeFocus || root.preset50Amount === root.burnAmount ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset50.pressed ? "#D95C1E" : root.preset50Amount === root.burnAmount ? "#F26A21" : preset50.hovered || preset50.activeFocus ? "#E7E1D8" : "#151515"
                    radius: 6
                }
            }

            Button {
                id: preset75

                activeFocusOnTab: root.hasLpTokens
                enabled: root.hasLpTokens
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("75%")

                Accessible.name: qsTr("Remove 75 percent")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.setBurnPercent(75)

                contentItem: Text {
                    color: preset75.enabled && (preset75.hovered || preset75.activeFocus || root.preset75Amount === root.burnAmount) ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset75.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset75.activeFocus || root.preset75Amount === root.burnAmount ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset75.pressed ? "#D95C1E" : root.preset75Amount === root.burnAmount ? "#F26A21" : preset75.hovered || preset75.activeFocus ? "#E7E1D8" : "#151515"
                    radius: 6
                }
            }

            Button {
                id: presetMax

                activeFocusOnTab: root.hasLpTokens
                enabled: root.hasLpTokens
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("MAX")

                Accessible.name: qsTr("Remove maximum LP balance")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.setBurnAmount(root.maxBurnAmount)

                contentItem: Text {
                    color: presetMax.enabled && (presetMax.hovered || presetMax.activeFocus || root.burnAmount === root.maxBurnAmount) ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: presetMax.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: presetMax.activeFocus || root.burnAmount === root.maxBurnAmount ? "#F26A21" : "#343434"
                    border.width: 1
                    color: presetMax.pressed ? "#D95C1E" : root.burnAmount === root.maxBurnAmount ? "#F26A21" : presetMax.hovered || presetMax.activeFocus ? "#E7E1D8" : "#151515"
                    radius: 6
                }
            }
        }

        ColumnLayout {
            spacing: 6

            Layout.fillWidth: true

            RowLayout {
                spacing: 8

                Layout.fillWidth: true

                Text {
                    color: "#A9A098"
                    font.pixelSize: 12
                    text: qsTr("Pool share to remove")

                    Layout.fillWidth: true
                }

                Text {
                    color: "#E7E1D8"
                    font.bold: true
                    font.pixelSize: 12
                    horizontalAlignment: Text.AlignRight
                    text: root.poolState.formatPercent(root.removePercent)

                    Layout.maximumWidth: 72
                }
            }

            Slider {
                id: burnSlider

                activeFocusOnTab: root.hasLpTokens
                enabled: root.hasLpTokens
                from: 0
                stepSize: 1
                to: 100
                value: root.removePercent

                Accessible.name: qsTr("Pool share to remove")
                Accessible.role: Accessible.Slider

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onMoved: root.setBurnPercent(Math.round(value))

                background: Rectangle {
                    color: "#343434"
                    implicitHeight: 4
                    radius: 2
                    x: burnSlider.leftPadding
                    y: burnSlider.topPadding + burnSlider.availableHeight / 2 - height / 2

                    width: burnSlider.availableWidth

                    Rectangle {
                        color: burnSlider.enabled ? "#F26A21" : "#56504A"
                        height: parent.height
                        radius: 2
                        width: burnSlider.visualPosition * parent.width
                    }
                }

                handle: Rectangle {
                    border.color: burnSlider.activeFocus ? "#E7E1D8" : "#F26A21"
                    border.width: 1
                    color: burnSlider.enabled ? "#F26A21" : "#56504A"
                    height: 18
                    radius: 9
                    width: 18
                    x: burnSlider.leftPadding + burnSlider.visualPosition * (burnSlider.availableWidth - width)
                    y: burnSlider.topPadding + burnSlider.availableHeight / 2 - height / 2
                }
            }
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Withdraw %1").arg(root.poolState.tokenA)
            value: root.poolState.formatTokenAmount(root.preview.withdrawA, root.poolState.tokenA)
            visible: root.burnAmount > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Withdraw %1").arg(root.poolState.tokenB)
            value: root.poolState.formatTokenAmount(root.preview.withdrawB, root.poolState.tokenB)
            visible: root.burnAmount > 0

            Layout.fillWidth: true
        }

        SlippageToleranceControl {
            tolerancePercent: root.slippageTolerancePercent

            Layout.fillWidth: true

            onToleranceChangeRequested: function (tolerancePercent) {
                root.slippageToleranceChangeRequested(tolerancePercent);
            }
        }

        SummaryRow {
            label: qsTr("Min %1 received").arg(root.poolState.tokenA)
            value: root.poolState.formatTokenAmount(root.minTokenAReceived, root.poolState.tokenA)
            visible: root.burnAmount > 0

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Min %1 received").arg(root.poolState.tokenB)
            value: root.poolState.formatTokenAmount(root.minTokenBReceived, root.poolState.tokenB)
            visible: root.burnAmount > 0

            Layout.fillWidth: true
        }

        Text {
            color: "#F08A76"
            font.pixelSize: 12
            lineHeight: 1.25
            text: qsTr("Minimum received is 0. Increase amount or lower slippage.")
            visible: root.minReceivedIsZero
            wrapMode: Text.WordWrap

            Layout.fillWidth: true
        }

        SummaryRow {
            estimated: true
            estimateHelp: root.estimateHelp
            label: qsTr("Position after")
            value: root.poolState.formatPoolShare(root.preview.newUserShare)
            visible: root.burnAmount > 0

            Layout.fillWidth: true
        }

        Button {
            id: submitButton

            activeFocusOnTab: root.hasLpTokens
            enabled: root.canSubmit
            focusPolicy: Qt.StrongFocus
            hoverEnabled: true
            text: root.submitButtonText

            Accessible.name: submitButton.text

            Layout.fillWidth: true
            Layout.minimumHeight: 44
            Layout.preferredHeight: 44

            onClicked: root.removeLiquidityRequested(root.submitSnapshot())

            contentItem: Text {
                color: submitButton.enabled ? "#151515" : "#7D756E"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 13
                horizontalAlignment: Text.AlignHCenter
                text: submitButton.text
                verticalAlignment: Text.AlignVCenter
            }

            background: Rectangle {
                border.color: submitButton.enabled ? "#F26A21" : "#343434"
                border.width: 1
                color: submitButton.enabled ? submitButton.pressed ? "#D95C1E" : submitButton.hovered || submitButton.activeFocus ? "#FF8A3D" : "#F26A21" : "#181818"
                radius: 6
            }
        }
    }

    function setBurnAmount(value) {
        root.burnAmount = root.poolState.clampBurnAmount(value);
    }

    function setBurnPercent(percent) {
        root.setBurnAmount(root.poolState.burnAmountForPercent(percent));
    }

    function resetForm() {
        root.setBurnAmount(0);
    }

    function submitSnapshot() {
        return {
            "action": "remove",
            "burnAmount": root.preview.burnedLp,
            "burnPercent": root.poolState.formatPercent(root.removePercent),
            "burnText": root.poolState.formatLpAmount(root.preview.burnedLp),
            "minTokenAReceived": root.poolState.formatTokenAmount(root.minTokenAReceived, root.poolState.tokenA),
            "minTokenBReceived": root.poolState.formatTokenAmount(root.minTokenBReceived, root.poolState.tokenB),
            "postRemovalShare": root.poolState.formatPoolShare(root.preview.newUserShare),
            "slippageTolerance": root.poolState.formatPercent(root.slippageTolerancePercent),
            "tokenA": root.poolState.tokenA,
            "tokenB": root.poolState.tokenB,
            "withdrawA": root.preview.withdrawA,
            "withdrawB": root.preview.withdrawB,
            "withdrawAText": root.poolState.formatTokenAmount(root.preview.withdrawA, root.poolState.tokenA),
            "withdrawBText": root.poolState.formatTokenAmount(root.preview.withdrawB, root.poolState.tokenB)
        };
    }
}
