pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

import Logos.Theme
import Logos.Wallet

// Self-contained navigation bar — styling is independent of any view's theme.
// Use currentIndex to read the active tab; tabChanged(index) fires on selection.
// A tab carrying `items` opens a dropdown instead of switching directly, and the
// chosen entry lands in currentSubIndex (always 0 for tabs without a menu).
Item {
    id: root

    property int currentIndex: 0
    property int currentSubIndex: 0

    readonly property var tabs: [
        { "label": qsTr("Trade"), "items": [] },
        { "label": qsTr("Explore"), "items": [] },
        { "label": qsTr("Pool"), "items": [qsTr("View positions"), qsTr("Create pool")] }
    ]

    // Wallet wiring, passed down from Main.qml.
    property var backend: null
    property var accountModel: null

    // Address of the account currently selected in the header control.
    readonly property string selectedAddress: accountControl.selectedAddress

    signal tabChanged(int index)

    function select(index, subIndex) {
        root.currentIndex = index
        root.currentSubIndex = subIndex
        root.tabChanged(index)
    }

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
                        required property var modelData

                        readonly property bool active: root.currentIndex === index
                        readonly property var items: modelData.items || []
                        readonly property bool hasMenu: tabButton.items.length > 0

                        // Tracks where the pointer is so the menu can stay open
                        // while it travels from the tab down into the menu.
                        property bool pointerOnTab: false
                        property bool pointerInMenu: false

                        height: 36
                        width:  tabLabel.implicitWidth + 28
                        radius: 18
                        color:  active ? Theme.palette.backgroundSecondary : "transparent"
                        border.width: activeFocus ? 1 : 0
                        border.color: Theme.palette.text
                        activeFocusOnTab: true
                        Accessible.role: Accessible.PageTab
                        Accessible.name: tabLabel.text

                        // A tab with a menu defers the switch to the chosen entry;
                        // one without goes straight to its page.
                        function activate() {
                            if (tabButton.hasMenu) {
                                tabButton.openMenu()
                                return
                            }
                            root.select(tabButton.index, 0)
                        }

                        function openMenu() {
                            if (!tabButton.hasMenu)
                                return
                            menuCloseTimer.stop()
                            tabMenu.open()
                        }

                        // Closing is deferred: crossing the gap between the tab and
                        // the menu leaves the pointer over neither for a moment, and
                        // closing on that would make the menu unreachable by mouse.
                        function scheduleMenuClose() {
                            if (tabButton.hasMenu)
                                menuCloseTimer.restart()
                        }

                        Timer {
                            id: menuCloseTimer

                            interval: 180
                            onTriggered: {
                                if (!tabButton.pointerOnTab && !tabButton.pointerInMenu)
                                    tabMenu.close()
                            }
                        }

                        Popup {
                            id: tabMenu

                            objectName: "navTabMenu%1".arg(tabButton.index)
                            y: tabButton.height + 6
                            width: 190
                            padding: 6
                            closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

                            background: Rectangle {
                                radius: 12
                                color: Theme.palette.background
                                border.color: Theme.palette.borderSecondary
                                border.width: 1
                            }

                            contentItem: Column {
                                spacing: 2

                                // A handler rather than a MouseArea: the entries
                                // below have their own hoverEnabled MouseAreas,
                                // which would steal a parent MouseArea's hover.
                                HoverHandler {
                                    onHoveredChanged: {
                                        tabButton.pointerInMenu = hovered
                                        if (hovered)
                                            menuCloseTimer.stop()
                                        else
                                            tabButton.scheduleMenuClose()
                                    }
                                }

                                Repeater {
                                    model: tabButton.items

                                    delegate: Rectangle {
                                        id: menuEntry

                                        required property int index
                                        required property string modelData

                                        readonly property bool current: root.currentIndex === tabButton.index
                                                                        && root.currentSubIndex === menuEntry.index

                                        objectName: "navMenuItem%1_%2".arg(tabButton.index).arg(menuEntry.index)
                                        width: tabMenu.availableWidth
                                        height: 36
                                        radius: 8
                                        color: entryMouse.containsMouse
                                               ? Theme.palette.backgroundSecondary : "transparent"

                                        Accessible.role: Accessible.MenuItem
                                        Accessible.name: menuEntry.modelData

                                        function activate() {
                                            tabMenu.close()
                                            root.select(tabButton.index, menuEntry.index)
                                        }

                                        Text {
                                            anchors.left: parent.left
                                            anchors.leftMargin: 10
                                            anchors.right: parent.right
                                            anchors.rightMargin: 10
                                            anchors.verticalCenter: parent.verticalCenter
                                            text: menuEntry.modelData
                                            color: menuEntry.current || entryMouse.containsMouse
                                                   ? Theme.palette.text : Theme.palette.textSecondary
                                            font.pixelSize: 14
                                            font.weight: menuEntry.current ? Font.Medium : Font.Normal
                                            elide: Text.ElideRight
                                        }

                                        MouseArea {
                                            id: entryMouse

                                            anchors.fill: parent
                                            hoverEnabled: true
                                            cursorShape: Qt.PointingHandCursor
                                            onClicked: menuEntry.activate()
                                        }
                                    }
                                }
                            }
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
                            text: tabButton.modelData.label
                            color: tabButton.active ? Theme.palette.text : Theme.palette.textSecondary
                            font.pixelSize: 14
                            font.weight: tabButton.active ? Font.Medium : Font.Normal

                            Behavior on color { ColorAnimation { duration: 150 } }
                        }

                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor

                            onEntered: {
                                tabButton.pointerOnTab = true
                                tabButton.openMenu()
                            }

                            onExited: {
                                tabButton.pointerOnTab = false
                                tabButton.scheduleMenuClose()
                            }

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
