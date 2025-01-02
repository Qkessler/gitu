use super::{
    checkout, commit, create_prompt_with_default, latest_local_branch, push, selected_rev, Action,
    OpTrait,
};
use crate::{git, items::TargetData, menu::arg::Arg, state::State, term::Term, Res};
use std::{convert::Infallible, fmt::Display, process::Command, rc::Rc, str::FromStr};

// key for merge and rebase: "-s"
// key for cherry-pick and revert: "=s"
// shortarg for merge and rebase: "-s"
// shortarg for cherry-pick and revert: none

#[derive(Debug)]
enum StrategyArgValue {
    Resolve,
    Recursive,
    Octopus,
    Ours,
    Subtree,
}

impl FromStr for StrategyArgValue {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            _ => Ok(StrategyArgValue::Ours),
        }
    }
}

pub(crate) fn init_args() -> Vec<Arg> {
    vec![
        Arg::new_flag("--ff-only", "Fast-forward only", false),
        Arg::new_flag("--no-ff", "No fast-forward", false),
        // FIXME: Include Strategy before merging.
        // Arg::new_arg("--strategy=", "Strategy", None, StrategyArgValue::from_str),
    ]
}

/// From `magit-merge.el`.
///
/// Ref: <https://github.com/magit/magit/blob/main/lisp/magit-merge.el>
///
/// ["Actions"
///  :if-not magit-merge-in-progress-p
///  [("m" "Merge"                  magit-merge-plain)
///   ("e" "Merge and edit message" magit-merge-editmsg)
///   ("n" "Merge but don't commit" magit-merge-nocommit)
///   ("a" "Absorb"                 magit-merge-absorb)]
///  [("p" "Preview merge"          magit-merge-preview)
///   ""
///   ("s" "Squash merge"           magit-merge-squash)
///   ("i" "Dissolve"               magit-merge-into)]]
#[derive(Clone, PartialOrd, Ord, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum MergeAction {
    Plain,
    Edit,
    NoCommit,
    Absorb,
    // FIXME: Implement Preview.
    Squash,
    Dissolve,
}

impl MergeAction {
    fn plain(state: &mut State, term: &mut Term, rev: &str) -> Res<()> {
        let mut cmd = Command::new("git");
        cmd.args(["merge"]);
        cmd.args(state.pending_menu.as_ref().unwrap().args());
        cmd.args([rev]);
        state.close_menu();
        state.run_cmd_async(term, &[], cmd)
    }

    fn edit(state: &mut State, term: &mut Term, rev: &str) -> Res<()> {
        let mut cmd = Command::new("git");
        cmd.args(["merge", "--edit"]);
        cmd.args(state.pending_menu.as_ref().unwrap().args());
        cmd.args([rev]);
        state.close_menu();
        state.run_cmd_interactive(term, cmd)
    }

    fn no_commit(state: &mut State, term: &mut Term, rev: &str) -> Res<()> {
        let mut cmd = Command::new("git");
        let args = state.pending_menu.as_ref().unwrap().args();
        cmd.args(["merge", "--no-commit"]);
        if !args.iter().any(|arg| arg == "--no-ff") {
            cmd.arg("--no-ff");
        }
        cmd.args(args);
        cmd.args([rev]);
        state.close_menu();
        state.run_cmd_interactive(term, cmd)
    }

    // FIXME: This implementation is unfinished. This is one of the most
    // complex merge commands, so leaving for one of the later implementations.
    //
    // Ref: <https://github.com/magit/magit/blob/main/lisp/magit-merge.el#L171>
    fn absorb(_state: &mut State, _term: &mut Term, _branch_name: &str) -> Res<()> {
        todo!()
    }

    fn squash(state: &mut State, term: &mut Term, rev: &str) -> Res<()> {
        let mut cmd = Command::new("git");
        cmd.args(["merge", "--squash"]);
        cmd.args(state.pending_menu.as_ref().unwrap().args());
        cmd.args([rev]);
        state.close_menu();
        state.run_cmd_async(term, &[], cmd)
    }

    /// Merge the current branch into another and remove the former.
    ///
    /// Before merging, force push the source branch to its push-remote,
    /// provided the respective remote branch already exists, ensuring
    /// that the respective pull-request (if any) won't get stuck on some
    /// obsolete version of the commits that are being merged.
    ///
    /// Ref: <https://github.com/magit/magit/blob/main/lisp/magit-merge.el#L131>
    fn dissolve(state: &mut State, term: &mut Term, destination_branch: &str) -> Res<()> {
        let upstream = git::upstream_branch_name(&state.repo)?;
        push::set_upstream_and_push(state, term, &upstream)?;
        checkout::checkout(state, term, destination_branch)?;
        match git::get_head(&state.repo) {
            Ok(ref name) => MergeAction::absorb(state, term, name),
            // Head is not a branch
            Err(_) => MergeAction::edit(
                state,
                term,
                &selected_rev(state).ok_or("Revision must be selected")?,
            ),
        }
    }
}

impl Display for MergeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MergeAction::Plain => "Merge",
            MergeAction::Edit => "Merge and edit message",
            MergeAction::NoCommit => "Merge but don't commit",
            MergeAction::Absorb => "Absorb",
            MergeAction::Squash => "Squash merge",
            MergeAction::Dissolve => "Dissolve",
        })
    }
}

impl OpTrait for MergeAction {
    fn get_action(&self, _target: Option<&TargetData>) -> Option<Action> {
        let action = match self {
            MergeAction::Plain => {
                create_prompt_with_default("Merge", MergeAction::plain, selected_rev, true)
            }
            MergeAction::Edit => {
                create_prompt_with_default("Merge", MergeAction::edit, selected_rev, true)
            }
            MergeAction::NoCommit => {
                create_prompt_with_default("Merge", MergeAction::no_commit, selected_rev, true)
            }
            // Absorb branch:
            //
            // master
            // merge_menu2
            //
            // It's actual behaviour is that it it does the merge WITHOUT
            // an edit commit and deletes the source branch, kinda like a
            // PR.
            MergeAction::Absorb => create_prompt_with_default(
                "Absorb branch",
                MergeAction::absorb,
                latest_local_branch,
                true,
            ),
            MergeAction::Squash => {
                create_prompt_with_default("Squash", MergeAction::squash, selected_rev, true)
            }
            MergeAction::Dissolve => create_prompt_with_default(
                // FIXME: We _should_ include the current branch
                // into the prompt. Today, we don't have that information
                // on `State`, nor we have access to the `State` on this
                // function.
                "Merge current branch into",
                MergeAction::dissolve,
                latest_local_branch,
                true,
            ),
        };

        Some(action)
    }

    // FIXME: This can be simplified by requiring a `Display`
    // bound on `OpTrait`.
    fn display(&self, _state: &State) -> String {
        self.to_string()
    }
}

/// ["Actions"
///  :if magit-merge-in-progress-p
///  ("m" "Commit merge" magit-commit-create)
///  ("a" "Abort merge"  magit-merge-abort)])
enum MergeState {
    Commit,
    Abort,
}

impl MergeState {
    fn abort(state: &mut State, term: &mut Term) -> Res<()> {
        let mut cmd = Command::new("git");
        cmd.args(["merge", "--abort"]);

        state.close_menu();
        state.run_cmd_interactive(term, cmd)?;
        Ok(())
    }
}

impl OpTrait for MergeState {
    fn get_action(&self, _target: Option<&TargetData>) -> Option<Action> {
        let action = match self {
            MergeState::Commit => commit::Commit::commit,
            MergeState::Abort => MergeState::abort,
        };

        Some(Rc::new(action))
    }

    fn display(&self, _state: &State) -> String {
        match self {
            MergeState::Commit => "commit".to_string(),
            MergeState::Abort => "abort".to_string(),
        }
    }
}
