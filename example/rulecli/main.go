package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/charmbracelet/bubbles/help"
	"github.com/charmbracelet/bubbles/key"
	"github.com/charmbracelet/bubbles/textarea"
	"github.com/charmbracelet/bubbles/textinput"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/qpoint-io/rulekit"
)

const (
	// Input modes
	modeRule = iota
	modeData
	modeResult
	modeHelp
)

const (
	bigQ = `
 .d88888b.  
d88P" "Y88b 
888     888 
888     888 
888     888 
888 Y8b 888 
Y88b.Y8b88P 
 "Y888888"  
       Y8b  
`

	bigQInfo = `
Rulekit of a package built at Qpoint
Come visit us at https://qpoint.io

Press Ctrl+P to see preloaded samples
`
)

// UI Styles
var (
	titleStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FAFAFA")).
			Background(lipgloss.Color("#7D56F4")).
			Padding(0, 1)

	headerStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#E9C46A")).
			MarginBottom(0)

	errorStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#FF0000"))

	helpStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#626262"))

	exampleStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#2A9D8F")).
			MarginTop(1)

	sectionStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#F1C40F")).
			MarginTop(1)

	focusedStyle = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#7D56F4")).
			Padding(1)

	blurredStyle = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#626262")).
			Padding(1)

	bigQStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#7D56F4")).
			Align(lipgloss.Center).
			AlignVertical(lipgloss.Center)

	resultSuccess = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#00FF00"))

	resultFail = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FF0000"))

	invalidDataStyle = lipgloss.NewStyle().
				Border(lipgloss.RoundedBorder()).
				BorderForeground(lipgloss.Color("#FF0000")).
				Padding(1)

	labelStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#7D56F4"))

	modalStyle = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#7D56F4")).
			Padding(1, 2).
			Width(80).
			Background(lipgloss.Color("#121212")).
			Foreground(lipgloss.Color("#FAFAFA"))
)

// Predefined examples
var (
	// Example rules
	exampleRules = []string{
		`domain matches /example\.com$/ and port == 8080`,
		`ip in 192.168.1.0/24 and method == "POST"`,
		`status_code >= 400 and status_code < 500`,
		`user_agent contains "Mozilla" and not path matches /\.(jpg|png|gif)$/`,
		`request_count > 100 and response_time > 500`,
	}

	// Example data sets
	exampleData = []string{
		`{"domain": "example.com", "port": 8080, "path": "/index.html"}`,
		`{"ip": "192.168.1.100", "method": "POST", "path": "/api/data"}`,
		`{"status_code": 404, "path": "/not-found.html"}`,
		`{"user_agent": "Mozilla/5.0", "path": "/document.pdf"}`,
		`{"request_count": 150, "response_time": 750}`,
	}
)

// keymap defines all keyboard shortcuts for the application
type keymap struct {
	Tab      key.Binding
	Enter    key.Binding
	Evaluate key.Binding
	LoadRule key.Binding
	LoadData key.Binding
	LoadPair key.Binding
	ClearAll key.Binding
	Back     key.Binding
	Quit     key.Binding
	ShowHelp key.Binding
}

func (k keymap) ShortHelp() []key.Binding {
	return []key.Binding{k.Tab, k.ShowHelp, k.Quit}
}

func (k keymap) FullHelp() [][]key.Binding {
	return [][]key.Binding{
		{k.Tab, k.Enter, k.Evaluate},
		{k.LoadRule, k.LoadData, k.LoadPair, k.ClearAll},
		{k.Back, k.Quit},
	}
}

// Default keymap
var keys = keymap{
	Tab: key.NewBinding(
		key.WithKeys("tab"),
		key.WithHelp("tab", "switch between rule and data"),
	),
	Enter: key.NewBinding(
		key.WithKeys("enter"),
		key.WithHelp("enter", "add new line in data"),
	),
	Evaluate: key.NewBinding(
		key.WithKeys("ctrl+e"),
		key.WithHelp("ctrl+e", "evaluate rule"),
	),
	LoadRule: key.NewBinding(
		key.WithKeys("ctrl+r"),
		key.WithHelp("ctrl+r", "load example rule (1-5)"),
	),
	LoadData: key.NewBinding(
		key.WithKeys("ctrl+d"),
		key.WithHelp("ctrl+d", "load example data (1-5)"),
	),
	LoadPair: key.NewBinding(
		key.WithKeys("ctrl+p"),
		key.WithHelp("ctrl+p", "load matching rule+data pair (1-5)"),
	),
	ClearAll: key.NewBinding(
		key.WithKeys("ctrl+x"),
		key.WithHelp("ctrl+x", "clear all inputs"),
	),
	Back: key.NewBinding(
		key.WithKeys("esc"),
		key.WithHelp("esc", "back to editing"),
	),
	Quit: key.NewBinding(
		key.WithKeys("ctrl+c"),
		key.WithHelp("ctrl+c", "quit"),
	),
	ShowHelp: key.NewBinding(
		key.WithKeys("ctrl+h"),
		key.WithHelp("ctrl+h", "show help"),
	),
}

// loadExampleMsg is a message for loading example rules or data
type loadExampleMsg struct {
	exampleType string // "rule", "data", or "pair"
	number      int    // 1-based index for the example
}

// model holds the application state
type model struct {
	mode            int
	ruleInput       textinput.Model
	dataInput       textarea.Model
	exampleNumInput textinput.Model
	pendingLoad     string // "rule", "data", or "pair"
	result          string
	resultError     string
	resultView      viewport.Model
	helpModalView   viewport.Model
	help            help.Model
	windowSize      tea.WindowSizeMsg
	validJSON       bool
}

// initialModel creates and initializes a new application model
func initialModel() model {
	ruleInput := textinput.New()
	ruleInput.Placeholder = "Enter a rule expression"
	ruleInput.Focus()
	ruleInput.CharLimit = 256
	ruleInput.Prompt = ""

	dataInput := textarea.New()
	dataInput.Placeholder = "Enter test data as JSON"
	dataInput.CharLimit = 1024
	dataInput.ShowLineNumbers = false

	exampleNumInput := textinput.New()
	exampleNumInput.Placeholder = "Enter example # (1-5)"
	exampleNumInput.Width = 5
	exampleNumInput.CharLimit = 1

	resultView := viewport.New(60, 20)
	helpModalView := viewport.New(80, 25)
	helpModel := help.New()

	return model{
		mode:            modeRule,
		ruleInput:       ruleInput,
		dataInput:       dataInput,
		exampleNumInput: exampleNumInput,
		resultView:      resultView,
		helpModalView:   helpModalView,
		help:            helpModel,
		validJSON:       true, // Assume valid JSON initially
	}
}

// Init initializes the model
func (m model) Init() tea.Cmd {
	return textinput.Blink
}

// Update handles messages and updates the model
func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	// var cmd tea.Cmd
	// var cmds []tea.Cmd

	switch msg := msg.(type) {
	case loadExampleMsg:
		return m.handleExampleLoad(msg)

	case tea.KeyMsg:
		// Handle pending example loads
		if m.pendingLoad != "" {
			return m.handlePendingLoadKeypress(msg)
		}

		// Handle keys based on mode
		switch {
		case key.Matches(msg, keys.Quit):
			return m, tea.Quit

		case key.Matches(msg, keys.Back):
			return m.handleBackKey()

		case key.Matches(msg, keys.Tab):
			return m.handleTabKey()

		case key.Matches(msg, keys.Evaluate):
			m.mode = modeResult
			m.evaluateRule()
			return m, nil

		case key.Matches(msg, keys.LoadRule):
			m.pendingLoad = "rule"
			return m, nil

		case key.Matches(msg, keys.LoadData):
			m.pendingLoad = "data"
			return m, nil

		case key.Matches(msg, keys.LoadPair):
			m.pendingLoad = "pair"
			return m, nil

		case key.Matches(msg, keys.ClearAll):
			m.ruleInput.SetValue("")
			m.dataInput.SetValue("")
			m.result = ""
			m.resultError = ""
			return m, nil

		case key.Matches(msg, keys.ShowHelp):
			m.mode = modeHelp
			m.helpModalView.SetContent(m.generateHelpContent())
			return m, nil
		}

	case tea.WindowSizeMsg:
		return m.handleWindowResize(msg)
	}

	// Update the appropriate component based on current mode
	return m.updateCurrentMode(msg)
}

// handleExampleLoad loads the selected example
func (m model) handleExampleLoad(msg loadExampleMsg) (tea.Model, tea.Cmd) {
	if msg.number < 1 || msg.number > len(exampleRules) {
		return m, nil
	}

	idx := msg.number - 1
	switch msg.exampleType {
	case "rule":
		m.ruleInput.SetValue(exampleRules[idx])
	case "data":
		m.dataInput.SetValue(exampleData[idx])
		m.prettyPrintDataInput()
		m.validateJSON()
	case "pair":
		m.ruleInput.SetValue(exampleRules[idx])
		m.dataInput.SetValue(exampleData[idx])
		m.prettyPrintDataInput()
		m.validateJSON()
	}

	// Reset pending load
	m.pendingLoad = ""
	m.exampleNumInput.Blur()

	// Return to previous mode
	if m.mode == modeRule {
		m.ruleInput.Focus()
	} else if m.mode == modeData {
		m.dataInput.Focus()
	}

	return m, nil
}

// handlePendingLoadKeypress processes keypresses during example selection
func (m model) handlePendingLoadKeypress(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "1", "2", "3", "4", "5":
		num, _ := strconv.Atoi(msg.String())
		pendingType := m.pendingLoad
		m.pendingLoad = ""
		return m, func() tea.Msg {
			return loadExampleMsg{exampleType: pendingType, number: num}
		}
	case "esc":
		// Cancel loading example
		m.pendingLoad = ""

		// Return to previous mode
		if m.mode == modeRule {
			m.ruleInput.Focus()
		} else if m.mode == modeData {
			m.dataInput.Focus()
		}

		return m, nil
	}
	return m, nil
}

// handleBackKey handles the ESC key press
func (m model) handleBackKey() (tea.Model, tea.Cmd) {
	if m.mode == modeResult {
		// Go back to rule input
		m.mode = modeRule
		m.ruleInput.Focus()
		return m, textinput.Blink
	} else if m.mode == modeHelp {
		// Return from help modal to previous mode
		m.mode = modeRule // Default to rule mode
		m.ruleInput.Focus()
		return m, nil
	}
	return m, nil
}

// handleTabKey handles switching between rule and data input
func (m model) handleTabKey() (tea.Model, tea.Cmd) {
	if m.mode == modeRule {
		m.mode = modeData
		m.ruleInput.Blur()
		m.dataInput.Focus()
		return m, textarea.Blink
	} else if m.mode == modeData {
		// Pretty print JSON before leaving data input
		m.prettyPrintDataInput()
		m.mode = modeRule
		m.dataInput.Blur()
		m.ruleInput.Focus()
		return m, textinput.Blink
	}
	return m, nil
}

// handleWindowResize updates components for a new window size
func (m model) handleWindowResize(msg tea.WindowSizeMsg) (tea.Model, tea.Cmd) {
	m.windowSize = msg

	// Resize all components based on new window size
	m.resizeInputs()

	// Update the help model width
	m.help.Width = msg.Width

	// Update content views
	if m.mode == modeResult {
		m.resultView.SetContent(m.generateResultContent())
	}

	// Update help modal content if displayed
	if m.mode == modeHelp {
		m.helpModalView.SetContent(m.generateHelpContent())
	}

	return m, nil
}

// updateCurrentMode updates the active component based on current mode
func (m model) updateCurrentMode(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd
	var cmds []tea.Cmd

	switch m.mode {
	case modeRule:
		m.ruleInput, cmd = m.ruleInput.Update(msg)
		cmds = append(cmds, cmd)
	case modeData:
		m.dataInput, cmd = m.dataInput.Update(msg)
		cmds = append(cmds, cmd)
		m.validateJSON()
	case modeResult:
		m.resultView, cmd = m.resultView.Update(msg)
		cmds = append(cmds, cmd)
	}

	return m, tea.Batch(cmds...)
}

// evaluateRule parses and evaluates the rule against the test data
func (m *model) evaluateRule() {
	// Reset previous results
	m.result = ""
	m.resultError = ""

	// Validate JSON first
	m.validateJSON()

	// Parse rule
	rule, err := rulekit.Parse(m.ruleInput.Value())
	if err != nil {
		m.resultError = fmt.Sprintf("Error parsing rule: %v", err)
		return
	}

	// Parse data
	var data map[string]any
	if err := json.Unmarshal([]byte(m.dataInput.Value()), &data); err != nil {
		m.resultError = fmt.Sprintf("Error parsing JSON data: %v", err)
		return
	}

	// Evaluate rule
	result := rule.Eval(data)

	// Format the result
	var sb strings.Builder

	// Show results first on the same line
	sb.WriteString(labelStyle.Render("Result: "))
	if result.Pass {
		sb.WriteString(resultSuccess.Render("PASS"))
	} else {
		sb.WriteString(resultFail.Render("FAIL"))
	}

	sb.WriteString("    ")
	sb.WriteString(labelStyle.Render("Strict Result: "))
	if result.PassStrict() {
		sb.WriteString(resultSuccess.Render("PASS"))
	} else {
		sb.WriteString(resultFail.Render("FAIL"))
	}
	sb.WriteString("\n\n")

	// Show rule next
	sb.WriteString(labelStyle.Render("Rule: "))
	sb.WriteString(fmt.Sprintf("%s\n\n", rule))

	// Show missing fields if any
	if len(result.MissingFields) > 0 {
		sb.WriteString(labelStyle.Render("Missing Fields: "))
		sb.WriteString(fmt.Sprintf("%v\n\n", result.MissingFields))
	}

	// Show test data last
	sb.WriteString(labelStyle.Render("Test Data:") + "\n")
	prettyJSON, err := json.MarshalIndent(data, "", "  ")
	if err != nil {
		sb.WriteString(m.dataInput.Value())
	} else {
		sb.Write(prettyJSON)
	}

	m.result = sb.String()

	// Update the result view content
	m.resultView.SetContent(m.generateResultContent())
}

// validateJSON checks if the current data input contains valid JSON
func (m *model) validateJSON() {
	// Skip validation if data is empty
	if strings.TrimSpace(m.dataInput.Value()) == "" {
		m.validJSON = true
		return
	}

	// Try to parse the JSON
	var data map[string]any
	m.validJSON = json.Unmarshal([]byte(m.dataInput.Value()), &data) == nil
}

// prettyPrintDataInput formats the JSON in the data input
func (m *model) prettyPrintDataInput() {
	// Skip if data is empty
	if strings.TrimSpace(m.dataInput.Value()) == "" {
		return
	}

	// Try to parse the JSON
	var data map[string]any
	if err := json.Unmarshal([]byte(m.dataInput.Value()), &data); err != nil {
		// Not valid JSON, leave as is
		m.validJSON = false
		return
	}

	// Format the JSON
	prettyJSON, err := json.MarshalIndent(data, "", "  ")
	if err != nil {
		// Error formatting, leave as is
		return
	}

	// Update the input with the pretty-printed JSON
	m.dataInput.SetValue(string(prettyJSON))
	m.validJSON = true
}

// generateResultContent creates the content for the result view
func (m *model) generateResultContent() string {
	if m.resultError != "" {
		return errorStyle.Render(m.resultError)
	}
	if m.result != "" {
		return m.result
	}
	// Use the centered purple bigQ with the additional text
	return bigQStyle.Render(bigQ + bigQInfo)
}

// resizeInputs adjusts the dimensions of inputs based on window size
func (m *model) resizeInputs() {
	if m.windowSize.Width == 0 {
		return // Skip if window size not yet set
	}

	// Calculate the left column width (40% of available space)
	contentWidth := m.windowSize.Width - 15
	leftColumnWidth := int(float64(contentWidth) * 0.4)
	rightColumnWidth := contentWidth - leftColumnWidth

	// Account for borders and padding
	inputWidth := leftColumnWidth - 4

	// Set input widths
	m.ruleInput.Width = inputWidth
	m.dataInput.SetWidth(inputWidth + 1)

	// Calculate available height for inputs
	// Account for: title (4), rule header (1), rule input (3), spacing (3), data header (1),
	// bottom spacing and help text (5)
	const (
		fixedVerticalElements = 17
		ruleInputHeight       = 3 // Fixed height for rule input
		minHeight             = 5 // Minimum height for components
	)

	// Calculate the height for data input to fill remaining space
	availableHeight := m.windowSize.Height - fixedVerticalElements
	dataInputHeight := availableHeight - ruleInputHeight

	// Ensure minimum height
	if dataInputHeight < minHeight {
		dataInputHeight = minHeight
	}

	// Set the data input height
	m.dataInput.SetHeight(dataInputHeight)

	// Make the results view use the available horizontal and vertical space
	// Set the width of the results view (account for padding and borders)
	m.resultView.Width = rightColumnWidth - 4

	// Account for: title (4), results header (1), bottom spacing and help text (5)
	const resultsFixedVerticalElements = 10
	resultsHeight := m.windowSize.Height - resultsFixedVerticalElements

	// Ensure minimum height
	if resultsHeight < minHeight {
		resultsHeight = minHeight
	}

	m.resultView.Height = resultsHeight - 2

	// Set help modal size - make it 80% of the window size
	m.helpModalView.Width = int(float64(m.windowSize.Width) * 0.8)
	m.helpModalView.Height = int(float64(m.windowSize.Height) * 0.8)
}

// View renders the UI
func (m model) View() string {
	var sb strings.Builder

	// Title
	sb.WriteString(titleStyle.Render("RuleKit Demo") + "\n\n")

	// If in help mode, show the help modal
	if m.mode == modeHelp {
		return m.renderHelpModal(&sb)
	}

	// Example selection prompt
	if m.pendingLoad != "" {
		return m.renderExampleSelection(&sb)
	}

	// Render the main UI with rule, data, and result panels
	return m.renderMainUI(&sb)
}

// renderHelpModal renders the help modal screen
func (m model) renderHelpModal(sb *strings.Builder) string {
	// Center the modal on screen
	helpViewWidth := m.helpModalView.Width + 4 // Account for padding and borders

	// Calculate horizontal padding to center the modal
	horizontalPadding := (m.windowSize.Width - helpViewWidth) / 2
	if horizontalPadding < 0 {
		horizontalPadding = 0
	}

	// Display the modal
	helpModalContent := modalStyle.Width(m.helpModalView.Width).Render(m.helpModalView.View())
	helpModalStyled := lipgloss.NewStyle().
		PaddingLeft(horizontalPadding).
		PaddingTop(2).
		Render(helpModalContent)

	sb.WriteString(helpModalStyled)

	// Add key hint at the bottom
	keyHint := lipgloss.NewStyle().
		Foreground(lipgloss.Color("#626262")).
		Italic(true).
		Align(lipgloss.Center).
		Width(m.windowSize.Width).
		Render("Press ESC to return")

	sb.WriteString("\n\n" + keyHint)

	return sb.String()
}

// renderExampleSelection renders the example selection screen
func (m model) renderExampleSelection(sb *strings.Builder) string {
	var promptType string
	switch m.pendingLoad {
	case "rule":
		promptType = "rule"
	case "data":
		promptType = "data"
	case "pair":
		promptType = "rule and data pair"
	}

	sb.WriteString(fmt.Sprintf("Choose example %s (1-5) or ESC to cancel:\n\n", promptType))

	// Display the available examples based on what's being loaded
	if m.pendingLoad == "rule" || m.pendingLoad == "pair" {
		sb.WriteString(sectionStyle.Render("Example Rules:") + "\n")
		for i, ex := range exampleRules {
			sb.WriteString(exampleStyle.Render(fmt.Sprintf("%d. %s", i+1, ex)) + "\n")
		}
		sb.WriteString("\n")
	}

	if m.pendingLoad == "data" || m.pendingLoad == "pair" {
		sb.WriteString(sectionStyle.Render("Example Test Data:") + "\n")
		for i, ex := range exampleData {
			sb.WriteString(exampleStyle.Render(fmt.Sprintf("%d. %s", i+1, ex)) + "\n")
		}
	}

	return sb.String()
}

// renderMainUI renders the main application UI with rule and data inputs and results
func (m model) renderMainUI(sb *strings.Builder) string {
	// Create left column with rule input and data input
	leftColumn := m.renderLeftColumn()

	// Create right column with results
	rightColumn := m.renderRightColumn()

	// Calculate column widths based on window size
	totalWidth := m.windowSize.Width
	if totalWidth == 0 {
		totalWidth = 100 // Default width if window size is not yet known
	}

	// Apply column layout
	contentWidth := totalWidth - 10 // Account for some padding
	leftColumnWidth := int(float64(contentWidth) * 0.4)
	rightColumnWidth := contentWidth - leftColumnWidth

	leftColumnStyled := lipgloss.NewStyle().Width(leftColumnWidth).Render(leftColumn)
	rightColumnStyled := lipgloss.NewStyle().
		Width(rightColumnWidth).
		Height(m.resultView.Height + 2). // Add 2 for the header and some padding
		Render(rightColumn)

	// Join columns horizontally
	contentArea := lipgloss.JoinHorizontal(lipgloss.Top, leftColumnStyled, rightColumnStyled)
	sb.WriteString(contentArea)

	// Help
	sb.WriteString("\n\n" + helpStyle.Render(m.help.View(keys)))

	return sb.String()
}

// renderLeftColumn renders the rule and data input column
func (m model) renderLeftColumn() string {
	// Rule input
	ruleBox := m.ruleInput.View()
	ruleContainer := headerStyle.Render("Rule Expression (Press Ctrl+P for samples)") + "\n"
	if m.mode == modeRule {
		ruleContainer += focusedStyle.Render(ruleBox)
	} else {
		ruleContainer += blurredStyle.Render(ruleBox)
	}

	// Data input
	dataBox := m.dataInput.View()
	dataContainer := headerStyle.Render("Test Data (JSON)") + "\n"

	if m.mode == modeData {
		if !m.validJSON && m.dataInput.Value() != "" {
			dataContainer += invalidDataStyle.Render(dataBox)
		} else {
			dataContainer += focusedStyle.Render(dataBox)
		}
	} else {
		if !m.validJSON && m.dataInput.Value() != "" {
			dataContainer += invalidDataStyle.Render(dataBox)
		} else {
			dataContainer += blurredStyle.Render(dataBox)
		}
	}

	// Combine rule and data into left column
	return lipgloss.JoinVertical(lipgloss.Left, ruleContainer, "\n", dataContainer)
}

// renderRightColumn renders the result column
func (m model) renderRightColumn() string {
	if m.mode == modeResult {
		resultsHeader := headerStyle.Render("Results (ESC to return)")
		resultsView := m.resultView.View()
		return lipgloss.JoinVertical(
			lipgloss.Left,
			resultsHeader,
			focusedStyle.Width(m.resultView.Width).Height(m.resultView.Height).Render(resultsView),
		)
	}

	resultsHeader := headerStyle.Render("Results (Press Ctrl+E to evaluate)")

	// Replace placeholder with centered, purple bigQ and additional text
	centeredBigQ := bigQStyle.
		Width(m.resultView.Width).
		Height(m.resultView.Height).
		Render(bigQ + bigQInfo)

	return lipgloss.JoinVertical(
		lipgloss.Left,
		resultsHeader,
		blurredStyle.Width(m.resultView.Width).Height(m.resultView.Height).Render(centeredBigQ),
	)
}

// generateHelpContent creates a nicely formatted help content for the modal
func (m *model) generateHelpContent() string {
	var sb strings.Builder

	sb.WriteString(titleStyle.Render("RuleKit Help") + "\n\n")

	// Basic Navigation
	sb.WriteString(headerStyle.Render("Basic Navigation") + "\n")
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "tab", "Switch between rule and data inputs"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+h", "Show this help screen"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+c", "Quit the application"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "esc", "Go back or close modal"))
	sb.WriteString("\n")

	// Evaluation
	sb.WriteString(headerStyle.Render("Evaluation") + "\n")
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+e", "Evaluate rule against data"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "enter", "Add new line in data input"))
	sb.WriteString("\n")

	// Loading Examples
	sb.WriteString(headerStyle.Render("Loading Examples") + "\n")
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+r", "Load example rule (1-5)"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+d", "Load example data (1-5)"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+p", "Load example rule+data pair (1-5)"))
	sb.WriteString(fmt.Sprintf("%-20s %s\n", "ctrl+x", "Clear all inputs"))
	sb.WriteString("\n\n")

	// Footer
	sb.WriteString("Press ESC to close this help screen")

	return sb.String()
}

func main() {
	model := initialModel()

	// Set a default window size to start with reasonable dimensions
	model.windowSize = tea.WindowSizeMsg{Width: 100, Height: 40}

	// Initialize component sizes based on default window size
	model.resizeInputs()

	// Ensure help view has correct width
	model.help.Width = model.windowSize.Width

	p := tea.NewProgram(model, tea.WithAltScreen())
	if _, err := p.Run(); err != nil {
		fmt.Printf("Error running program: %v\n", err)
		os.Exit(1)
	}
}
