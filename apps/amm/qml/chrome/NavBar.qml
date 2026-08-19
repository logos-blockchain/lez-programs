pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Layouts 1.15

import Logos.Theme
import Logos.Wallet

// Self-contained navigation bar — styling is independent of any view's theme.
// Use currentIndex to read the active tab; tabChanged(index) fires on selection.
Item {
    id: root

    property int currentIndex: 0
    readonly property var tabs: [qsTr("Trade"), qsTr("Liquidity"), qsTr("Pools")]

    // Wallet wiring, passed down from Main.qml.
    property var backend: null
    property var accountModel: null

    // Address of the account currently selected in the header control.
    readonly property string selectedAddress: accountControl.selectedAddress

    signal tabChanged(int index)

    implicitHeight: 56

    Rectangle {
        anchors.fill: parent
        color: Theme.palette.background

        // Bottom separator
        Rectangle {
            anchors.left:   parent.left
            anchors.right:  parent.right
            anchors.bottom: parent.bottom
            height: 1
            color: Theme.palette.borderSecondary
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin:  20
            anchors.rightMargin: 20
            spacing: 4

            // App identity
            Text {
                text: qsTr("Logos AMM")
                color: Theme.palette.text
                font.pixelSize: 17
                font.weight: Font.Bold
            }

            Item { Layout.fillWidth: true }

            // Tab pills
            Row {
                Accessible.role: Accessible.PageTabList
                Accessible.name: qsTr("Primary navigation")

                spacing: 4

                Repeater {
                    model: root.tabs

                    delegate: Rectangle {
                        id: tabButton

                        required property int index
                        required property string modelData

                        readonly property bool active: root.currentIndex === index

                        height: 36
                        width:  tabLabel.implicitWidth + 28
                        radius: 18
                        color:  active ? Theme.palette.backgroundSecondary : "transparent"
                        border.width: activeFocus ? 1 : 0
                        border.color: Theme.palette.text
                        activeFocusOnTab: true
                        Accessible.role: Accessible.PageTab
                        Accessible.name: tabLabel.text

                        function activate() {
                            root.currentIndex = index
                            root.tabChanged(index)
                        }

                        Behavior on color { ColorAnimation { duration: 150 } }

                        Keys.onReturnPressed: function(event) {
                            tabButton.activate()
                            event.accepted = true
                        }

                        Keys.onEnterPressed: function(event) {
                            tabButton.activate()
                            event.accepted = true
                        }

                        Keys.onSpacePressed: function(event) {
                            tabButton.activate()
                            event.accepted = true
                        }

                        Text {
                            id: tabLabel
                            anchors.centerIn: parent
                            text: tabButton.modelData
                            color: tabButton.active ? Theme.palette.text : Theme.palette.textSecondary
                            font.pixelSize: 14
                            font.weight: tabButton.active ? Font.Medium : Font.Normal

                            Behavior on color { ColorAnimation { duration: 150 } }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                tabButton.forceActiveFocus()
                                tabButton.activate()
                            }
                        }
                    }
                }
            }

            // Wallet / account control on the far right.
            WalletControl {
                id: accountControl
                Layout.leftMargin: 12
                wallet: root.backend
                accountModel: root.accountModel
                viewportWidth: root.width
                watchCall: function(result, success, failure) {
                    logos.watch(result, success, failure)
                }
            }
        }
    }
}
