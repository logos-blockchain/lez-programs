pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic

ComboBox {
    id: root

    required property var theme
    property var labelForOption: function(option) { return String(option || "") }

    implicitHeight: 34
    leftPadding: 10
    rightPadding: 28
    topPadding: 0
    bottomPadding: 0
    hoverEnabled: true
    activeFocusOnTab: true
    focusPolicy: Qt.StrongFocus

    contentItem: Text {
        leftPadding: root.leftPadding
        rightPadding: root.rightPadding
        text: root.displayText
        color: root.enabled ? root.theme.colors.textPrimary
                            : root.theme.colors.textPlaceholder
        font.pixelSize: 11
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideMiddle
    }

    indicator: Text {
        x: root.width - width - 10
        y: Math.round((root.height - height) / 2)
        text: "\u25BE"
        color: root.enabled ? root.theme.colors.textSecondary
                            : root.theme.colors.textPlaceholder
        font.pixelSize: 10
    }

    background: Rectangle {
        radius: 7
        color: !root.enabled
               ? root.theme.colors.panelBg
               : root.down
                 ? root.theme.colors.selection
                 : root.hovered || root.activeFocus
                   ? root.theme.colors.panelHoverBg
                   : root.theme.colors.panelBg
        border.color: root.activeFocus ? root.theme.colors.ctaBg
                                       : root.theme.colors.borderStrong
        border.width: 1
    }

    delegate: ItemDelegate {
        id: optionDelegate

        required property int index
        required property var modelData

        width: ListView.view ? ListView.view.width : root.width
        height: 34
        hoverEnabled: true
        highlighted: root.highlightedIndex === optionDelegate.index

        contentItem: Text {
            leftPadding: 8
            rightPadding: 8
            text: root.labelForOption(optionDelegate.modelData)
            color: root.theme.colors.textPrimary
            font.pixelSize: 11
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideMiddle
        }

        background: Rectangle {
            radius: 5
            color: optionDelegate.highlighted || optionDelegate.hovered
                   ? root.theme.colors.panelHoverBg : "transparent"
        }
    }

    popup: Popup {
        y: root.height + 4
        width: root.width
        implicitHeight: Math.min(contentItem.implicitHeight + topPadding + bottomPadding,
                                 204)
        padding: 4
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: root.delegateModel
            currentIndex: root.highlightedIndex
            highlightMoveDuration: 0
            ScrollIndicator.vertical: ScrollIndicator { }
        }

        background: Rectangle {
            radius: 7
            color: root.theme.colors.cardBg
            border.color: root.theme.colors.borderStrong
            border.width: 1
        }
    }
}
