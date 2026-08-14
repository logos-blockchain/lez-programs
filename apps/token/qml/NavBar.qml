pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Layouts 1.15

import Logos.Theme

// Shared wallet UI module (apps/common/wallet-ui). Imported by relative path
// because the ui-host only searches the runtime's own QML import path, not the
// app's plugin dir. Once the module ships as a compiled qrc module (see PR #228)
// this becomes `import Logos.Wallet`.
import "Logos/Wallet"

// Self-contained navigation bar — styling is independent of any view's theme.
// Use currentIndex to read the active tab; tabChanged(index) fires on selection.
Item {
    id: root

    property int currentIndex: 0
    readonly property var tabs: [qsTr("Create"), qsTr("Inspect")]

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

            // Tab pills
            Row {
                spacing: 4

                Repeater {
                    model: root.tabs

                    delegate: Rectangle {
                        id: tabPill

                        required property int index
                        required property var modelData
                        readonly property int tabIndex: index
                        readonly property bool active: root.currentIndex === tabIndex

                        height: 36
                        width: tabLabel.implicitWidth + 28
                        radius: 18
                        color: active ? Theme.palette.backgroundSecondary : "transparent"
                        border.color: activeFocus ? Theme.palette.overlayOrange : "transparent"
                        border.width: activeFocus ? 1 : 0
                        activeFocusOnTab: true
                        Accessible.name: qsTr("Open %1").arg(tabPill.modelData)
                        Accessible.role: Accessible.Button
                        Accessible.onPressAction: tabPill.activate()

                        function activate() {
                            root.currentIndex = tabPill.tabIndex;
                            root.tabChanged(tabPill.tabIndex);
                        }

                        Keys.onReturnPressed: tabPill.activate()
                        Keys.onSpacePressed: tabPill.activate()

                        Behavior on color {
                            ColorAnimation {
                                duration: 150
                            }
                        }

                        Text {
                            id: tabLabel
                            anchors.centerIn: parent
                            text: tabPill.modelData
                            color: tabPill.active ? Theme.palette.text : Theme.palette.textSecondary
                            font.pixelSize: 14
                            font.weight: tabPill.active ? Font.Medium : Font.Normal

                            Behavior on color {
                                ColorAnimation {
                                    duration: 150
                                }
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: tabPill.activate()
                        }
                    }
                }
            }

            // Wallet / account control on the far right.
            WalletControl {
                id: accountControl
                Layout.leftMargin: 12
                backend: root.backend
                accountModel: root.accountModel
            }
        }
    }
}
