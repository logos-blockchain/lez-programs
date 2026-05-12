import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property var theme
    property string label: ""
    property string amount: ""
    property string usdValue: ""
    property var token: null
    property bool active: true

    signal tokenClicked()
    signal inputEdited(string newValue)

    Binding {
        target: tiInput
        property: "text"
        value: root.amount
    }

    radius: 16
    color: root.active ? theme.colors.inputBg : theme.colors.panelBg
    implicitHeight: 110

    Behavior on color { ColorAnimation { duration: 300 } }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 14
        anchors.bottomMargin: 14
        spacing: 8

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4

            Text {
                text: root.label
                color: theme.colors.textSecondary
                font.pixelSize: 14
            }

            Item {
                Layout.fillWidth: true
                height: 44

                TextInput {
                    id: tiInput
                    anchors.fill: parent
                    color: root.active ? theme.colors.textPrimary : theme.colors.textSecondary
                    font.pixelSize: 36
                    font.weight: Font.Bold
                    selectionColor: theme.colors.selection
                    clip: true
                    onTextEdited: root.inputEdited(text)
                    validator: RegularExpressionValidator {
                        regularExpression: /^[0-9]*\.?[0-9]*$/
                    }
                }

                Text {
                    anchors.fill: parent
                    text: "0"
                    color: theme.colors.textPlaceholder
                    font: tiInput.font
                    visible: tiInput.text === "" && !tiInput.activeFocus
                    verticalAlignment: Text.AlignVCenter
                }
            }

            Text {
                text: root.usdValue
                color: theme.colors.textSecondary
                font.pixelSize: 13
                visible: root.usdValue !== ""
            }
        }

        Rectangle {
            height: 40
            radius: 20
            color: tokenBtnHover.containsMouse ? theme.colors.panelHoverBg : theme.colors.panelBg
            implicitWidth: tokenBtnRow.implicitWidth + 24
            Behavior on color { ColorAnimation { duration: 120 } }

            RowLayout {
                id: tokenBtnRow
                anchors.centerIn: parent
                spacing: 6

                Rectangle {
                    width: 24; height: 24; radius: 12
                    color: root.token ? root.token.color : theme.colors.noTokenCircle
                    visible: root.token !== null
                    Text {
                        anchors.centerIn: parent
                        text: root.token ? root.token.letter : ""
                        color: "#ffffff"
                        font.pixelSize: 10
                        font.weight: Font.Bold
                    }
                }

                Text {
                    text: root.token ? root.token.symbol : "Select token"
                    color: theme.colors.textPrimary
                    font.pixelSize: 15
                    font.weight: root.token ? Font.Medium : Font.Normal
                }

                Text {
                    text: "▼"
                    color: theme.colors.textSecondary
                    font.pixelSize: 10
                }
            }

            MouseArea {
                id: tokenBtnHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.tokenClicked()
            }
        }
    }
}
