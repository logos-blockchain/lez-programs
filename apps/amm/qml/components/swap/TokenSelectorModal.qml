import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property var theme
    property var tokens: []
    property string searchText: ""

    signal tokenSelected(var token)

    visible: false

    function open() {
        root.visible = true
        searchText = ""
        searchField.text = ""
        searchField.forceActiveFocus()
    }

    function close() {
        root.visible = false
    }

    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.4)
        MouseArea { anchors.fill: parent; onClicked: root.close() }
    }

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(480, root.width - 32)
        height: Math.min(600, root.height - 64)
        radius: 24
        color: theme.colors.cardBg
        border.color: theme.colors.border
        border.width: 1

        Behavior on color { ColorAnimation { duration: 300 } }

        MouseArea { anchors.fill: parent; onClicked: {} }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            RowLayout {
                Layout.fillWidth: true
                Text {
                    Layout.fillWidth: true
                    text: "Select a token"
                    color: theme.colors.textPrimary
                    font.pixelSize: 18
                    font.weight: Font.Bold
                }
                Rectangle {
                    width: 32; height: 32; radius: 16
                    color: closeHover.containsMouse ? theme.colors.panelHoverBg : theme.colors.panelBg
                    Behavior on color { ColorAnimation { duration: 120 } }
                    Text { anchors.centerIn: parent; text: "✕"; color: theme.colors.textSecondary; font.pixelSize: 14 }
                    MouseArea {
                        id: closeHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.close()
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: 48
                radius: 16
                color: theme.colors.inputBg
                border.color: searchField.activeFocus ? theme.colors.borderStrong : theme.colors.border
                border.width: 1
                Behavior on border.color { ColorAnimation { duration: 150 } }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 14
                    anchors.rightMargin: 14
                    spacing: 8
                    Text { text: "⌕"; color: theme.colors.textSecondary; font.pixelSize: 20 }
                    TextInput {
                        id: searchField
                        Layout.fillWidth: true
                        color: theme.colors.textPrimary
                        font.pixelSize: 15
                        selectionColor: theme.colors.selection
                        onTextChanged: root.searchText = text
                        Text {
                            anchors.fill: parent
                            text: "Search tokens"
                            color: theme.colors.textPlaceholder
                            font: searchField.font
                            visible: searchField.text === "" && !searchField.activeFocus
                            verticalAlignment: Text.AlignVCenter
                        }
                    }
                }
            }

            Text { text: "Popular tokens"; color: theme.colors.textSecondary; font.pixelSize: 13 }

            Flow {
                Layout.fillWidth: true
                spacing: 8
                Repeater {
                    model: root.tokens.slice(0, 5)
                    delegate: Rectangle {
                        height: 40
                        radius: 20
                        color: pillHover.containsMouse ? theme.colors.panelHoverBg : theme.colors.panelBg
                        border.color: theme.colors.border
                        border.width: 1
                        width: pillRow.implicitWidth + 24
                        Behavior on color { ColorAnimation { duration: 120 } }
                        RowLayout {
                            id: pillRow
                            anchors.centerIn: parent
                            spacing: 6
                            Rectangle {
                                width: 22; height: 22; radius: 11
                                color: modelData.color
                                Text { anchors.centerIn: parent; text: modelData.letter; color: "#ffffff"; font.pixelSize: 10; font.weight: Font.Bold }
                            }
                            Text { text: modelData.symbol; color: theme.colors.textPrimary; font.pixelSize: 13; font.weight: Font.Medium }
                        }
                        MouseArea {
                            id: pillHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.tokenSelected(modelData)
                        }
                    }
                }
            }

            Text { text: "Tokens by 24H volume"; color: theme.colors.textSecondary; font.pixelSize: 13 }

            ListView {
                id: tokenList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 2
                model: root.tokens.filter(function(t) {
                    if (root.searchText === "") return true
                    var q = root.searchText.toLowerCase()
                    return t.symbol.toLowerCase().indexOf(q) !== -1 ||
                           t.name.toLowerCase().indexOf(q) !== -1
                })
                delegate: TokenListItem {
                    width: tokenList.width
                    theme: root.theme
                    tokenName: modelData.name
                    tokenSymbol: modelData.symbol
                    tokenAddress: modelData.address
                    tokenColor: modelData.color
                    tokenLetter: modelData.letter
                    onClicked: root.tokenSelected(modelData)
                }
            }
        }
    }
}
