import QtQuick 2.15
import QtQuick.Layouts 1.15

// Self-contained navigation bar — styling is independent of any view's theme.
// Use currentIndex to read the active tab; tabChanged(index) fires on selection.
Item {
    id: root

    property int currentIndex: 0
    readonly property var tabs: ["Trade", "Liquidity"]

    signal tabChanged(int index)

    implicitHeight: 56

    Rectangle {
        anchors.fill: parent
        color: "#ffffff"

        // Bottom separator
        Rectangle {
            anchors.left:   parent.left
            anchors.right:  parent.right
            anchors.bottom: parent.bottom
            height: 1
            color: Qt.rgba(0, 0, 0, 0.08)
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin:  20
            anchors.rightMargin: 20
            spacing: 4

            // App identity
            Text {
                text: "Logos AMM"
                color: "#111111"
                font.pixelSize: 17
                font.weight: Font.Bold
            }

            Item { Layout.fillWidth: true }

            // Tab pills
            Row {
                spacing: 4

                Repeater {
                    model: root.tabs

                    delegate: Rectangle {
                        readonly property bool active: root.currentIndex === index

                        height: 36
                        width:  tabLabel.implicitWidth + 28
                        radius: 18
                        color:  active ? "#111111" : "transparent"

                        Behavior on color { ColorAnimation { duration: 150 } }

                        Text {
                            id: tabLabel
                            anchors.centerIn: parent
                            text:        modelData
                            color:       active ? "#ffffff" : "#666666"
                            font.pixelSize: 14
                            font.weight: active ? Font.Medium : Font.Normal

                            Behavior on color { ColorAnimation { duration: 150 } }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.currentIndex = index
                                root.tabChanged(index)
                            }
                        }
                    }
                }
            }
        }
    }
}
