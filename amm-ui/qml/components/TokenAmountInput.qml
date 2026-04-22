import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property alias text: amountField.text
    property string balance: ""
    property string errorText: ""
    property string helperText: ""
    property string label: ""
    property string token: ""

    signal editingChanged(string value)
    signal maxClicked

    color: "#151515"
    implicitHeight: content.implicitHeight + 20
    radius: 8
    border.color: root.errorText.length > 0 ? "#D85F4B" : amountField.activeFocus ? "#F26A21" : "#343434"
    border.width: 1

    Accessible.name: root.label
    Accessible.role: Accessible.EditableText

    ColumnLayout {
        id: content

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
                text: root.label

                Layout.fillWidth: true
            }

            Text {
                color: "#E7E1D8"
                elide: Text.ElideRight
                font.bold: true
                font.pixelSize: 12
                horizontalAlignment: Text.AlignRight
                text: root.token

                Layout.maximumWidth: 76
            }
        }

        RowLayout {
            spacing: 8

            Layout.fillWidth: true

            TextField {
                id: amountField

                activeFocusOnTab: true
                color: "#E7E1D8"
                font.bold: true
                font.pixelSize: 18
                inputMethodHints: Qt.ImhFormattedNumbersOnly
                placeholderText: qsTr("0")
                selectByMouse: true
                selectedTextColor: "#151515"
                selectionColor: "#F26A21"
                validator: RegularExpressionValidator {
                    regularExpression: /[0-9]*([.][0-9]*)?/
                }

                Accessible.name: root.label

                Layout.fillWidth: true
                Layout.minimumHeight: 44

                onTextEdited: root.editingChanged(text)

                background: Rectangle {
                    border.color: amountField.activeFocus ? "#F26A21" : "#343434"
                    border.width: 1
                    color: amountField.activeFocus ? "#1F1B18" : "#101010"
                    radius: 6
                }
            }

            Button {
                id: maxButton

                activeFocusOnTab: true
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                text: qsTr("MAX")

                Accessible.name: qsTr("Use maximum %1 balance").arg(root.token)

                Layout.minimumHeight: 44
                Layout.preferredWidth: 58

                onClicked: root.maxClicked()

                contentItem: Text {
                    color: maxButton.activeFocus || maxButton.hovered ? "#151515" : "#F26A21"
                    font.bold: true
                    font.pixelSize: 11
                    horizontalAlignment: Text.AlignHCenter
                    text: maxButton.text
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    border.color: "#F26A21"
                    border.width: 1
                    color: maxButton.pressed ? "#D95C1E" : maxButton.hovered || maxButton.activeFocus ? "#F26A21" : "#201712"
                    radius: 6
                }
            }
        }

        RowLayout {
            spacing: 8

            Layout.fillWidth: true

            Text {
                color: root.errorText.length > 0 ? "#F08A76" : root.helperText.length > 0 ? "#F26A21" : "#A9A098"
                elide: Text.ElideRight
                font.pixelSize: 11
                text: root.errorText.length > 0 ? root.errorText : root.helperText
                visible: text.length > 0

                Layout.fillWidth: true
            }

            Text {
                color: "#A9A098"
                elide: Text.ElideRight
                font.pixelSize: 11
                horizontalAlignment: Text.AlignRight
                text: qsTr("Balance %1").arg(root.balance)

                Layout.alignment: Qt.AlignRight
                Layout.maximumWidth: 150
            }
        }
    }
}
