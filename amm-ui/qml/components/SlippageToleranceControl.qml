import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property real tolerancePercent: 0.5
    property string customText: ""
    readonly property string thresholdText: root.tolerancePercent <= 1 ? qsTr("Standard slippage") : root.tolerancePercent <= 5 ? qsTr("Higher slippage") : qsTr("High slippage risk")
    readonly property string thresholdIcon: root.tolerancePercent <= 1 ? "i" : root.tolerancePercent <= 5 ? "!" : "!!"

    signal toleranceChangeRequested(real tolerancePercent)

    color: "#151515"
    implicitHeight: content.implicitHeight + 20
    radius: 8
    border.color: customField.activeFocus ? "#F26A21" : "#343434"
    border.width: 1

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
        anchors.margins: 10
        spacing: 8

        Text {
            color: "#A9A098"
            font.pixelSize: 12
            text: qsTr("Slippage tolerance")

            Layout.fillWidth: true
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
                    color: preset01.hovered || preset01.activeFocus || preset01.selected ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset01.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset01.activeFocus || preset01.selected ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset01.pressed ? "#D95C1E" : preset01.selected ? "#F26A21" : preset01.hovered || preset01.activeFocus ? "#E7E1D8" : "#101010"
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
                    color: preset05.hovered || preset05.activeFocus || preset05.selected ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset05.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset05.activeFocus || preset05.selected ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset05.pressed ? "#D95C1E" : preset05.selected ? "#F26A21" : preset05.hovered || preset05.activeFocus ? "#E7E1D8" : "#101010"
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
                    color: preset10.hovered || preset10.activeFocus || preset10.selected ? "#151515" : "#A9A098"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: preset10.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: preset10.activeFocus || preset10.selected ? "#F26A21" : "#343434"
                    border.width: 1
                    color: preset10.pressed ? "#D95C1E" : preset10.selected ? "#F26A21" : preset10.hovered || preset10.activeFocus ? "#E7E1D8" : "#101010"
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

        RowLayout {
            spacing: 6

            Layout.fillWidth: true

            Text {
                color: root.tolerancePercent <= 1 ? "#8FD6A4" : root.tolerancePercent <= 5 ? "#F2B366" : "#F08A76"
                font.bold: true
                font.pixelSize: 11
                horizontalAlignment: Text.AlignHCenter
                text: root.thresholdIcon

                Layout.preferredWidth: 18
            }

            Text {
                color: root.tolerancePercent <= 1 ? "#8FD6A4" : root.tolerancePercent <= 5 ? "#F2B366" : "#F08A76"
                font.pixelSize: 11
                text: root.thresholdText

                Layout.fillWidth: true
            }
        }

        Text {
            color: "#A9A098"
            font.pixelSize: 11
            text: qsTr("Allowed range: 0.01% to 50%.")
            wrapMode: Text.WordWrap

            Layout.fillWidth: true
        }
    }
}
