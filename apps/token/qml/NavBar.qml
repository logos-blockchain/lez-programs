pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

import Logos.Controls
import Logos.Theme
import Logos.Wallet

// Self-contained navigation bar — styling is independent of any view's theme.
Item {
    id: root

    property alias currentIndex: navigationTabs.currentIndex

    // Wallet wiring, passed down from Main.qml.
    property var backend: null
    property var accountModel: null

    implicitHeight: 56

    Rectangle {
        anchors.fill: parent
        color: Theme.palette.background

        // Bottom separator
        Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: 1
            color: Theme.palette.borderSecondary
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 20
            anchors.rightMargin: 20
            spacing: 4

            // App identity
            Text {
                text: qsTr("Logos Token")
                color: Theme.palette.text
                font.pixelSize: 17
                font.weight: Font.Bold
            }

            Item {
                Layout.fillWidth: true
            }

            LogosTabBar {
                id: navigationTabs

                Layout.preferredWidth: 180

                LogosTabButton {
                    text: qsTr("Create")
                    Accessible.name: qsTr("Open Create")
                }

                LogosTabButton {
                    text: qsTr("Inspect")
                    Accessible.name: qsTr("Open Inspect")
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
