import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property var theme
    property string tokenName: ""
    property string tokenSymbol: ""
    property string tokenAddress: ""
    property string tokenColor: "#627eea"
    property string tokenLetter: ""

    signal clicked()

    implicitHeight: 56

    Rectangle {
        anchors.fill: parent
        radius: 12
        color: hoverArea.containsMouse ? theme.colors.panelBg : "transparent"
        Behavior on color { ColorAnimation { duration: 120 } }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 12

            Rectangle {
                width: 36; height: 36; radius: 18
                color: root.tokenColor
                Text {
                    anchors.centerIn: parent
                    text: root.tokenLetter
                    color: "#ffffff"
                    font.pixelSize: 14
                    font.weight: Font.Bold
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                Text {
                    text: root.tokenName
                    color: theme.colors.textPrimary
                    font.pixelSize: 15
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                RowLayout {
                    spacing: 6
                    Text { text: root.tokenSymbol; color: theme.colors.textSecondary; font.pixelSize: 12 }
                    Text {
                        text: root.tokenAddress !== ""
                              ? root.tokenAddress.substring(0, 6) + "..." + root.tokenAddress.slice(-4)
                              : ""
                        color: theme.colors.textPlaceholder
                        font.pixelSize: 12
                    }
                }
            }
        }

        MouseArea {
            id: hoverArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.clicked()
        }
    }
}
