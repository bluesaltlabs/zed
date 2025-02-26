use super::stack_frame_list::{StackFrameList, StackFrameListEvent};
use dap::StackFrameId;
use editor::{Editor, EditorEvent};
use gpui::{
    actions, anchored, deferred, list, AnyElement, Context, Entity, FocusHandle, Focusable, Hsla,
    ListState, MouseDownEvent, Point, Subscription,
};
use project::debugger::session::{Scope, Session};
use std::collections::HashMap;
use ui::{prelude::*, ContextMenu, ListItem};

actions!(variable_list, [ExpandSelectedEntry, CollapseSelectedEntry]);

type _IsToggled = bool;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Variable {
    dap: dap::Variable,
    depth: usize,
    is_expanded: bool,
    children: Vec<Variable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopeState {
    variables: Vec<Variable>,
    dap: dap::Scope,
    is_expanded: bool,
}

enum VariableListEntry {
    _Scope(ScopeState),
    _Variable(Variable),
}

pub struct VariableList {
    _scopes: HashMap<StackFrameId, Vec<ScopeState>>,
    _selected_stack_frame_id: Option<StackFrameId>,
    list: ListState,
    _session: Entity<Session>,
    _selection: Option<VariableListEntry>,
    open_context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl VariableList {
    pub fn new(
        session: Entity<Session>,
        stack_frame_list: Entity<StackFrameList>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak_variable_list = cx.weak_entity();
        let focus_handle = cx.focus_handle();

        let list = ListState::new(
            0,
            gpui::ListAlignment::Top,
            px(1000.),
            move |ix, _window, cx| {
                weak_variable_list
                    .upgrade()
                    .map(|var_list| var_list.update(cx, |this, cx| this.render_entry(ix, cx)))
                    .unwrap_or(div().into_any())
            },
        );

        let set_variable_editor = cx.new(|cx| Editor::single_line(window, cx));

        cx.subscribe(
            &set_variable_editor,
            |_this: &mut Self, _, event: &EditorEvent, _cx| {
                if *event == EditorEvent::Blurred {
                    // this.cancel_set_variable_value(cx);
                }
            },
        )
        .detach();

        let _subscriptions =
            vec![cx.subscribe(&stack_frame_list, Self::handle_stack_frame_list_events)];

        Self {
            list,
            _session: session,
            focus_handle,
            _subscriptions,
            _selected_stack_frame_id: None,
            _selection: None,
            open_context_menu: None,
            _scopes: Default::default(),
        }
    }

    fn _build_entries(
        &mut self,
        stack_frame_id: StackFrameId,
        _cx: &mut Context<Self>,
    ) -> Vec<VariableListEntry> {
        let ret = vec![];

        let Some(_scopes) = self._scopes.get_mut(&stack_frame_id) else {
            // self.session
            //     .update(cx, |session, cx| session.scopes(stack_frame_id, cx));
            return ret;
        };

        fn inner(
            _this: &mut VariableList,
            _variable_reference: u64,
            _entries: &mut Vec<VariableListEntry>,
            _cx: &mut Context<VariableList>,
        ) {
        }

        // for scope in scopes {
        //     for variable in scope.variables {
        //         ret.push(VariableListEntry::Variable(variable));
        //     }
        // }

        todo!()
    }

    fn handle_stack_frame_list_events(
        &mut self,
        _: Entity<StackFrameList>,
        event: &StackFrameListEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            StackFrameListEvent::SelectedStackFrameChanged(stack_frame_id) => {
                self.handle_selected_stack_frame_changed(*stack_frame_id, cx);
            }
        }
    }

    fn handle_selected_stack_frame_changed(
        &mut self,
        _stack_frame_id: StackFrameId,
        _cx: &mut Context<Self>,
    ) {
        // if self.scopes.contains_key(&stack_frame_id) {
        //     return self.build_entries(true, cx);
        // }

        // self.fetch_variables_task = Some(cx.spawn(|this, mut cx| async move {
        //     let task = this.update(&mut cx, |variable_list, cx| {
        //         variable_list.fetch_variables_for_stack_frame(stack_frame_id, cx)
        //     })?;

        //     let (scopes, variables) = task.await?;

        //     this.update(&mut cx, |variable_list, cx| {
        //         variable_list.scopes.insert(stack_frame_id, scopes);

        //         for (scope_id, variables) in variables.into_iter() {
        //             let mut variable_index = ScopeVariableIndex::new();
        //             variable_index.add_variables(scope_id, variables);

        //             variable_list
        //                 .variables
        //                 .insert((stack_frame_id, scope_id), variable_index);
        //         }

        //         variable_list.build_entries(true, cx);

        //         variable_list.fetch_variables_task.take();
        //     })
        // }));
    }

    // pub fn completion_variables(&self, cx: &mut Context<Self>) -> Vec<VariableContainer> {
    //     let stack_frame_id = self
    //         .stack_frame_list
    //         .update(cx, |this, cx| this.get_main_stack_frame_id(cx));

    //     self.variables
    //         .range((stack_frame_id, u64::MIN)..(stack_frame_id, u64::MAX))
    //         .flat_map(|(_, containers)| containers.variables.iter().cloned())
    //         .collect()
    // }

    fn render_entry(&mut self, _ix: usize, _cx: &mut Context<Self>) -> AnyElement {
        // let Some(entry) = self.entries.get(ix) else {
        //     debug_panic!("Trying to render entry in variable list that has an out of bounds index");
        //     return div().into_any_element();
        // };

        return div().into_any_element();

        // let entry = &entries[ix];
        // match entry {
        //     VariableListEntry::Scope(scope) => self.render_scope(&scope, false, cx), // todo(debugger) pass a valid value for is selected
        //     VariableListEntry::Variable(variable) => {
        //         self.render_variable(&variable, false, cx)
        //     }
        // }
    }

    fn _toggle_variable(
        &mut self,
        _scope: &Scope,
        _variable: &Variable,
        _depth: usize,
        _cx: &mut Context<Self>,
    ) {
    }

    // fn select_first(&mut self, _: &SelectFirst, _window: &mut Window, cx: &mut Context<Self>) {
    //     let stack_frame_id = self.stack_frame_list.read(cx).current_stack_frame_id();
    //     if let Some(entries) = self.entries.get(&stack_frame_id) {
    //         self.selection = entries.first().cloned();
    //         cx.notify();
    //     };
    // }

    // fn select_last(&mut self, _: &SelectLast, _window: &mut Window, cx: &mut Context<Self>) {
    //     let stack_frame_id = self.stack_frame_list.read(cx).current_stack_frame_id();
    //     if let Some(entries) = self.entries.get(&stack_frame_id) {
    //         self.selection = entries.last().cloned();
    //         cx.notify();
    //     };
    // }

    // // fn select_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
    // //     if let Some(selection) = &self.selection {
    // //         let stack_frame_id = self.stack_frame_list.read(cx).current_stack_frame_id();
    // //         if let Some(entries) = self.entries.get(&stack_frame_id) {
    // //             if let Some(ix) = entries.iter().position(|entry| entry == selection) {
    // //                 self.selection = entries.get(ix.saturating_sub(1)).cloned();
    // //                 cx.notify();
    // //             }
    // //         }
    // //     } else {
    // //         self.select_first(&SelectFirst, window, cx);
    // //     }
    // // }

    // fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
    //     if let Some(selection) = &self.selection {
    //         let stack_frame_id = self.stack_frame_list.read(cx).current_stack_frame_id();
    //         if let Some(entries) = self.entries.get(&stack_frame_id) {
    //             if let Some(ix) = entries.iter().position(|entry| entry == selection) {
    //                 self.selection = entries.get(ix + 1).cloned();
    //                 cx.notify();
    //             }
    //         }
    //     } else {
    //         self.select_first(&SelectFirst, window, cx);
    //     }
    // }

    fn _collapse_selected_entry(
        &mut self,
        _: &CollapseSelectedEntry,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // if let Some(selection) = &self.selection {
        //     match selection {
        //         VariableListEntry::Scope(scope) => {
        //             let entry_id = &OpenEntry::Scope {
        //                 name: scope.name.clone(),
        //             };

        //             if self.open_entries.binary_search(entry_id).is_err() {
        //                 self.select_prev(&SelectPrev, window, cx);
        //             } else {
        //                 self.toggle_entry(entry_id, cx);
        //             }
        //         }
        //         VariableListEntry::Variable {
        //             depth,
        //             variable,
        //             scope,
        //             ..
        //         } => {
        //             let entry_id = &OpenEntry::Variable {
        //                 depth: *depth,
        //                 name: variable.name.clone(),
        //                 scope_name: scope.name.clone(),
        //             };

        //             if self.open_entries.binary_search(entry_id).is_err() {
        //                 self.select_prev(&SelectPrev, window, cx);
        //             } else {
        //                 // todo
        //             }
        //         }
        //         VariableListEntry::SetVariableEditor { .. } => {}
        //     }
        // }
    }

    fn _expand_selected_entry(
        &mut self,
        _: &ExpandSelectedEntry,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {

        // todo(debugger) Implement expand_selected_entry
        // if let Some(selection) = &self.selection {
        //     match selection {
        //         VariableListEntry::Scope(scope) => {
        //             let entry_id = &OpenEntry::Scope {
        //                 name: scope.name.clone(),
        //             };

        //             if self.open_entries.binary_search(entry_id).is_ok() {
        //                 self.select_next(&SelectNext, window, cx);
        //             } else {
        //                 self.toggle_entry(entry_id, cx);
        //             }
        //         }
        //         VariableListEntry::Variable {
        //             depth,
        //             variable,
        //             scope,
        //             ..
        //         } => {
        //             let entry_id = &OpenEntry::Variable {
        //                 depth: *depth,
        //                 name: variable.dap.name.clone(),
        //                 scope_name: scope.name.clone(),
        //             };

        //             if self.open_entries.binary_search(entry_id).is_ok() {
        //                 self.select_next(&SelectNext, window, cx);
        //             } else {
        //                 // self.toggle_variable(&scope.clone(), &variable.clone(), *depth, cx);
        //             }
        //         }
        //         VariableListEntry::SetVariableEditor { .. } => {}
        //     }
        // }
    }

    #[track_caller]
    #[cfg(any(test, feature = "test-support"))]
    pub fn assert_visual_entries(&self, expected: Vec<&str>, cx: &Context<Self>) {
        unimplemented!("Will finish after refactor is done");
        // const INDENT: &'static str = "    ";

        // let stack_frame_id = self.stack_frame_list.read(cx).current_stack_frame_id();
        // let entries = self.entries.get(&stack_frame_id).unwrap();

        // let mut visual_entries = Vec::with_capacity(entries.len());
        // for entry in entries {
        //     let is_selected = Some(entry) == self.selection.as_ref();

        //     match entry {
        //         VariableListEntry::Scope(scope) => {
        //             let is_expanded = self
        //                 .open_entries
        //                 .binary_search(&OpenEntry::Scope {
        //                     name: scope.name.clone(),
        //                 })
        //                 .is_ok();

        //             visual_entries.push(format!(
        //                 "{} {}{}",
        //                 if is_expanded { "v" } else { ">" },
        //                 scope.name,
        //                 if is_selected { " <=== selected" } else { "" }
        //             ));
        //         }
        //         VariableListEntry::SetVariableEditor { depth, state } => {
        //             visual_entries.push(format!(
        //                 "{}  [EDITOR: {}]{}",
        //                 INDENT.repeat(*depth),
        //                 state.name,
        //                 if is_selected { " <=== selected" } else { "" }
        //             ));
        //         }
        //         VariableListEntry::Variable {
        //             depth,
        //             variable,
        //             scope,
        //             ..
        //         } => {
        //             let is_expanded = self
        //                 .open_entries
        //                 .binary_search(&OpenEntry::Variable {
        //                     depth: *depth,
        //                     name: variable.name.clone(),
        //                     scope_name: scope.name.clone(),
        //                 })
        //                 .is_ok();

        //             visual_entries.push(format!(
        //                 "{}{} {}{}",
        //                 INDENT.repeat(*depth),
        //                 if is_expanded { "v" } else { ">" },
        //                 variable.name,
        //                 if is_selected { " <=== selected" } else { "" }
        //             ));
        //         }
        //     };
        // }

        // pretty_assertions::assert_eq!(expected, visual_entries);
    }

    #[allow(clippy::too_many_arguments)]
    fn _render_variable(
        &self,
        variable: &Variable,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disclosed = variable.is_expanded;

        let colors = _get_entry_color(cx);
        let bg_hover_color = if !is_selected {
            colors.hover
        } else {
            colors.default
        };
        let border_color = if is_selected {
            colors.marked_active
        } else {
            colors.default
        };

        div()
            .id(SharedString::from(format!(
                "variable-{}-{}",
                variable.dap.name, variable.depth
            )))
            .group("variable_list_entry")
            .border_1()
            .border_r_2()
            .border_color(border_color)
            .h_4()
            .size_full()
            .hover(|style| style.bg(bg_hover_color))
            .on_click(cx.listener({
                // let scope = scope.clone();
                // let variable = variable.clone();
                move |_this, _, _window, _cx| {
                    // this.selection = Some(VariableListEntry::Variable {
                    //     depth,
                    //     has_children,
                    //     container_reference,
                    //     scope: scope.clone(),
                    //     variable: variable.clone(),
                    // });
                    // cx.notify();
                }
            }))
            .child(
                ListItem::new(SharedString::from(format!(
                    "variable-item-{}-{}",
                    variable.dap.name, variable.depth
                )))
                .selectable(false)
                .indent_level(variable.depth as usize)
                .indent_step_size(px(20.))
                .always_show_disclosure_icon(true)
                .toggle(disclosed)
                // .when(
                //     variable.dap.variables_reference > 0,
                //     |list_item| {
                //         list_item.on_toggle(cx.listener({
                //             let variable = variable.clone();
                //             move |this, _, _window, cx| {
                //                 this.session.update(cx, |session, cx| {
                //                     session.variables(thread_id, stack_frame_id, variables_reference, cx)
                //                 })
                //                 this.toggle_variable(&scope, &variable, depth, cx)
                //             }
                //         }))
                //     },
                // )
                .on_secondary_mouse_down(cx.listener({
                    // let scope = scope.clone();
                    // let variable = variable.clone();
                    move |_this, _event: &MouseDownEvent, _window, _cx| {

                        // todo(debugger): Get this working
                        // this.deploy_variable_context_menu(
                        //     container_reference,
                        //     &scope,
                        //     &variable,
                        //     event.position,
                        //     window,
                        //     cx,
                        // )
                    }
                }))
                .child(
                    h_flex()
                        .gap_1()
                        .text_ui_sm(cx)
                        .child(variable.dap.name.clone())
                        .child(
                            div()
                                .text_ui_xs(cx)
                                .text_color(cx.theme().colors().text_muted)
                                .child(variable.dap.value.replace("\n", " ").clone()),
                        ),
                ),
            )
            .into_any()
    }

    fn _render_scope(
        &self,
        scope: &ScopeState,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let element_id = scope.dap.variables_reference;

        // todo(debugger) set this based on the scope being toggled or not
        let disclosed = true;

        let colors = _get_entry_color(cx);
        let bg_hover_color = if !is_selected {
            colors.hover
        } else {
            colors.default
        };
        let border_color = if is_selected {
            colors.marked_active
        } else {
            colors.default
        };

        div()
            .id(element_id as usize)
            .group("variable_list_entry")
            .border_1()
            .border_r_2()
            .border_color(border_color)
            .flex()
            .w_full()
            .h_full()
            .hover(|style| style.bg(bg_hover_color))
            .on_click(cx.listener({
                move |_this, _, _window, cx| {
                    cx.notify();
                }
            }))
            .child(
                ListItem::new(SharedString::from(format!(
                    "scope-{}",
                    scope.dap.variables_reference
                )))
                .selectable(false)
                .indent_level(1)
                .indent_step_size(px(20.))
                .always_show_disclosure_icon(true)
                .toggle(disclosed)
                .child(div().text_ui(cx).w_full().child(scope.dap.name.clone())),
            )
            .into_any()
    }
}

impl Focusable for VariableList {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for VariableList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // todo(debugger): We are reconstructing the variable list list state every frame
        // which is very bad!! We should only reconstruct the variable list state when necessary.
        // Will fix soon
        div()
            .key_context("VariableList")
            .id("variable-list")
            .group("variable-list")
            .size_full()
            .track_focus(&self.focus_handle(cx))
            // .on_action(cx.listener(Self::select_first))
            // .on_action(cx.listener(Self::select_last))
            // .on_action(cx.listener(Self::select_prev))
            // .on_action(cx.listener(Self::select_next))
            // .on_action(cx.listener(Self::expand_selected_entry))
            // .on_action(cx.listener(Self::collapse_selected_entry))
            //
            .on_action(
                cx.listener(|_this, _: &editor::actions::Cancel, _window, _cx| {

                    // this.cancel_set_variable_value(cx)
                }),
            )
            .child(list(self.list.clone()).gap_1_5().size_full())
            .children(self.open_context_menu.as_ref().map(|(menu, position, _)| {
                deferred(
                    anchored()
                        .position(*position)
                        .anchor(gpui::Corner::TopLeft)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
    }
}

struct _EntryColors {
    default: Hsla,
    hover: Hsla,
    marked_active: Hsla,
}

fn _get_entry_color(cx: &Context<VariableList>) -> _EntryColors {
    let colors = cx.theme().colors();

    _EntryColors {
        default: colors.panel_background,
        hover: colors.ghost_element_hover,
        marked_active: colors.ghost_element_selected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_initial_variables_to_index() {
        unimplemented!("This test is commented out")
        // let mut index = ScopeVariableIndex::new();

        // assert_eq!(index.variables(), vec![]);
        // assert_eq!(index.fetched_ids, HashSet::default());

        // let variable1 = VariableContainer {
        //     variable: Variable {
        //         name: "First variable".into(),
        //         value: "First variable".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 0,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // let variable2 = VariableContainer {
        //     variable: Variable {
        //         name: "Second variable with child".into(),
        //         value: "Second variable with child".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 2,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // let variable3 = VariableContainer {
        //     variable: Variable {
        //         name: "Third variable".into(),
        //         value: "Third variable".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 0,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // index.add_variables(
        //     1,
        //     vec![variable1.clone(), variable2.clone(), variable3.clone()],
        // );

        // assert_eq!(
        //     vec![variable1.clone(), variable2.clone(), variable3.clone()],
        //     index.variables(),
        // );
        // assert_eq!(HashSet::from([1]), index.fetched_ids,);
    }

    /// This covers when you click on a variable that has a nested variable
    /// We correctly insert the variables right after the variable you clicked on
    #[test]
    fn test_add_sub_variables_to_index() {
        unimplemented!("This test hasn't been refactored yet")
        // let mut index = ScopeVariableIndex::new();

        // assert_eq!(index.variables(), vec![]);

        // let variable1 = VariableContainer {
        //     variable: Variable {
        //         name: "First variable".into(),
        //         value: "First variable".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 0,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // let variable2 = VariableContainer {
        //     variable: Variable {
        //         name: "Second variable with child".into(),
        //         value: "Second variable with child".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 2,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // let variable3 = VariableContainer {
        //     variable: Variable {
        //         name: "Third variable".into(),
        //         value: "Third variable".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 0,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // index.add_variables(
        //     1,
        //     vec![variable1.clone(), variable2.clone(), variable3.clone()],
        // );

        // assert_eq!(
        //     vec![variable1.clone(), variable2.clone(), variable3.clone()],
        //     index.variables(),
        // );
        // assert_eq!(HashSet::from([1]), index.fetched_ids);

        // let variable4 = VariableContainer {
        //     variable: Variable {
        //         name: "Fourth variable".into(),
        //         value: "Fourth variable".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 0,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // let variable5 = VariableContainer {
        //     variable: Variable {
        //         name: "Five variable".into(),
        //         value: "Five variable".into(),
        //         type_: None,
        //         presentation_hint: None,
        //         evaluate_name: None,
        //         variables_reference: 0,
        //         named_variables: None,
        //         indexed_variables: None,
        //         memory_reference: None,
        //     },
        //     depth: 1,
        //     container_reference: 1,
        // };

        // index.add_variables(2, vec![variable4.clone(), variable5.clone()]);

        // assert_eq!(
        //     vec![
        //         variable1.clone(),
        //         variable2.clone(),
        //         variable4.clone(),
        //         variable5.clone(),
        //         variable3.clone(),
        //     ],
        //     index.variables(),
        // );
        // assert_eq!(index.fetched_ids, HashSet::from([1, 2]));
    }
}
