import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property real tolerancePercent: 0.5
    property string customText: ""
    readonly property string thresholdText: root.tolerancePercent <= 1 ? qsTr("Standard slippage") : root.tolerancePercent <= 5 ? qsTr("Higher slippage") : qsTr("High slippage risk")
    readonly property string thresholdIcon: root.tolerancePercent <= 1 ? "i" : root.tolerancePercent <= 5 ? "!" : "!!"

    signal toleranceChangeRequested(real tolerancePercent)

    implicitHeight: content.implicitHeight

    Component.onCompleted: root.restoreCustomText()

    onTolerancePercentChanged: {
        if (!customField.activeFocus) {
            root.restoreCustomText();
        }
    }

    function formatTolerance(value) {
        const amount = Number(value) || 0;
        return amount.toFixed(2).replace(/0+$/, "").replace(/[.]$/, "");
    }

    function restoreCustomText() {
        root.customText = root.formatTolerance(root.tolerancePercent);
    }

    function clampTolerance(value) {
        return Math.max(0.01, Math.min(50, Number(value) || 0));
    }

    function commitPreset(value) {
        const nextValue = root.clampTolerance(value);
        root.customText = root.formatTolerance(nextValue);
        root.toleranceChangeRequested(nextValue);
    }

    function commitCustom() {
        const parsed = Number(root.customText);

        if (root.customText.length === 0 || !isFinite(parsed) || parsed < 0) {
            root.restoreCustomText();
            return;
        }

        root.commitPreset(parsed);
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        spacing: 6

        RowLayout {
            spacing: 8

            Layout.fillWidth: true

            Text {
                color: "#A9A098"
                font.pixelSize: 12
                text: qsTr("Slippage tolerance")

                Layout.fillWidth: true
            }

            Text {
                color: root.tolerancePercent <= 1 ? "#8FD6A4" : root.tolerancePercent <= 5 ? "#F2B366" : "#F08A76"
                font.bold: true
                font.pixelSize: 11
                horizontalAlignment: Text.AlignRight
                text: root.thresholdText

                Layout.maximumWidth: 150
            }
        }

        RowLayout {
            spacing: 6

            Layout.fillWidth: true

            Button {
                id: preset01

                readonly property real presetValue: 0.1
                readonly property bool selected: Math.abs(root.tolerancePercent - presetValue) < 0.000001

                activeFocusOnTab: true
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("0.1%")

                Accessible.name: qsTr("Set slippage tolerance to 0.1 percent")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.commitPreset(presetValue)

                contentItem: Text {
                    color: preset01.selected ? "#F2D8C7" : preset01.hovered || preset01.activeFocus ? "#E7E1D8" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset01.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset01.activeFocus || preset01.selected ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset01.pressed ? "#2A1D16" : preset01.selected ? "#211914" : preset01.hovered || preset01.activeFocus ? "#202020" : "#101010"
                    radius: 6
                }
            }

            Button {
                id: preset05

                readonly property real presetValue: 0.5
                readonly property bool selected: Math.abs(root.tolerancePercent - presetValue) < 0.000001

                activeFocusOnTab: true
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("0.5%")

                Accessible.name: qsTr("Set slippage tolerance to 0.5 percent")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.commitPreset(presetValue)

                contentItem: Text {
                    color: preset05.selected ? "#F2D8C7" : preset05.hovered || preset05.activeFocus ? "#E7E1D8" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset05.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset05.activeFocus || preset05.selected ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset05.pressed ? "#2A1D16" : preset05.selected ? "#211914" : preset05.hovered || preset05.activeFocus ? "#202020" : "#101010"
                    radius: 6
                }
            }

            Button {
                id: preset10

                readonly property real presetValue: 1.0
                readonly property bool selected: Math.abs(root.tolerancePercent - presetValue) < 0.000001

                activeFocusOnTab: true
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("1.0%")

                Accessible.name: qsTr("Set slippage tolerance to 1.0 percent")

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onClicked: root.commitPreset(presetValue)

                contentItem: Text {
                    color: preset10.selected ? "#F2D8C7" : preset10.hovered || preset10.activeFocus ? "#E7E1D8" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset10.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset10.activeFocus || preset10.selected ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset10.pressed ? "#2A1D16" : preset10.selected ? "#211914" : preset10.hovered || preset10.activeFocus ? "#202020" : "#101010"
                    radius: 6
                }
            }

            Rectangle {
                color: customField.activeFocus ? "#1F1B18" : "#101010"
                radius: 6
                border.color: customField.activeFocus ? "#F26A21" : "#343434"
                border.width: 1

                Layout.minimumHeight: 44
                Layout.preferredWidth: 88

                RowLayout {
                    spacing: 4

                    anchors {
                        fill: parent
                        leftMargin: 8
                        rightMargin: 8
                    }

                    TextField {
                        id: customField

                        activeFocusOnTab: true
                        color: "#E7E1D8"
                        font.bold: true
                        font.pixelSize: 12
                        horizontalAlignment: Text.AlignRight
                        inputMethodHints: Qt.ImhFormattedNumbersOnly
                        placeholderText: qsTr("0.5")
                        selectByMouse: true
                        selectedTextColor: "#151515"
                        selectionColor: "#F26A21"
                        text: root.customText
                        validator: RegularExpressionValidator {
                            regularExpression: /[0-9]*([.][0-9]*)?/
                        }

                        Accessible.name: qsTr("Custom slippage tolerance percent")

                        Layout.fillWidth: true
                        Layout.minimumHeight: 42

                        onEditingFinished: root.commitCustom()
                        onTextEdited: root.customText = text
                        Keys.onEscapePressed: {
                            root.restoreCustomText();
                            customField.focus = false;
                        }

                        background: Item {}
                    }

                    Text {
                        color: "#A9A098"
                        font.bold: true
                        font.pixelSize: 12
                        text: qsTr("%")
                        verticalAlignment: Text.AlignVCenter

                        Layout.preferredWidth: 10
                    }
                }
            }
        }
    }
}
