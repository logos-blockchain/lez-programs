import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Vertical progress rail for the pool-creation flow (Uniswap-style): numbered
// steps connected by a line, with the active step highlighted and completed
// steps marked done. Read currentStep to drive which step is active. Clicking an
// already-reached step (index <= currentStep) emits stepClicked so the page can
// navigate back to it.
Item {
    id: root

    property int currentStep: 0
    readonly property var steps: [
        { title: qsTr("Select token pair"), subtitle: qsTr("Pick the two tokens for the pool.") },
        { title: qsTr("Deposit amounts"),   subtitle: qsTr("Set the initial liquidity.") }
    ]

    signal stepClicked(int index)

    implicitWidth: 240
    implicitHeight: column.implicitHeight

    ColumnLayout {
        id: column
        anchors.fill: parent
        spacing: 0

        Repeater {
            model: root.steps

            delegate: Item {
                id: stepItem

                readonly property bool active: index === root.currentStep
                readonly property bool done: index < root.currentStep
                readonly property bool last: index === root.steps.length - 1
                // Only steps already reached can be clicked (no jumping ahead).
                readonly property bool reachable: index <= root.currentStep

                Layout.fillWidth: true
                implicitHeight: stepRow.implicitHeight

                RowLayout {
                    id: stepRow
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    spacing: Theme.spacing.medium

                    // Indicator: numbered dot + connector line down to the next dot.
                    Item {
                        Layout.preferredWidth: 28
                        Layout.fillHeight: true

                        Rectangle {
                            id: dot
                            width: 28
                            height: 28
                            radius: 14
                            color: (stepItem.active || stepItem.done) ? Theme.palette.primary : Theme.palette.backgroundSecondary
                            border.width: 1
                            border.color: (stepItem.active || stepItem.done) ? Theme.palette.primary : Theme.palette.border

                            LogosText {
                                anchors.centerIn: parent
                                text: stepItem.done ? "✓" : (index + 1)
                                font.pixelSize: Theme.typography.secondaryText
                                font.bold: true
                                color: (stepItem.active || stepItem.done) ? Theme.palette.background : Theme.palette.textSecondary
                            }
                        }
                        Rectangle {
                            visible: !stepItem.last
                            width: 2
                            anchors.top: dot.bottom
                            anchors.bottom: parent.bottom
                            anchors.horizontalCenter: dot.horizontalCenter
                            color: stepItem.done ? Theme.palette.primary : Theme.palette.border
                        }
                    }

                    // Step text.
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.bottomMargin: stepItem.last ? 0 : Theme.spacing.xlarge
                        spacing: 2

                        LogosText {
                            text: modelData.title
                            font.pixelSize: Theme.typography.primaryText
                            font.bold: stepItem.active
                            color: (stepItem.active || stepItem.done) ? Theme.palette.text : Theme.palette.textSecondary
                        }
                        LogosText {
                            Layout.fillWidth: true
                            text: modelData.subtitle
                            wrapMode: Text.WordWrap
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    enabled: stepItem.reachable
                    cursorShape: stepItem.reachable ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onClicked: root.stepClicked(index)
                }
            }
        }
    }
}
