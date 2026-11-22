use std::error::Error;
use std::fmt;

#[derive(Clone, Debug)]
pub struct LodLevel<T> {
    asset: T,
    max_distance: f32,
    max_distance_squared: f32,
}

impl<T> LodLevel<T> {
    pub fn new(asset: T, max_distance: f32) -> Self {
        Self {
            asset,
            max_distance,
            max_distance_squared: max_distance * max_distance,
        }
    }

    pub fn asset(&self) -> &T {
        &self.asset
    }

    pub fn max_distance(&self) -> f32 {
        self.max_distance
    }
}

#[derive(Clone, Debug)]
pub struct LodProfile<T> {
    levels: Vec<LodLevel<T>>,
    hysteresis: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct LodChoice<'a, T> {
    pub index: usize,
    pub level: &'a LodLevel<T>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LodState {
    current: Option<usize>,
    initialized: bool,
}

impl LodState {
    pub fn current_level(&self) -> Option<usize> {
        self.current
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LodError {
    EmptyProfile,
    InvalidDistance,
    InvalidHysteresis,
    InvalidLevelDistance { index: usize },
    UnorderedLevelDistances { index: usize },
}

impl fmt::Display for LodError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProfile => formatter.write_str("a LOD profile needs at least one level"),
            Self::InvalidDistance => {
                formatter.write_str("LOD selection distance must be finite and non-negative")
            }
            Self::InvalidHysteresis => {
                formatter.write_str("LOD hysteresis must be finite and in the range [0, 0.5)")
            }
            Self::InvalidLevelDistance { index } => {
                write!(
                    formatter,
                    "LOD level {index} needs a finite positive distance"
                )
            }
            Self::UnorderedLevelDistances { index } => {
                write!(
                    formatter,
                    "LOD level {index} must be farther than the previous level"
                )
            }
        }
    }
}

impl Error for LodError {}

impl<T> LodProfile<T> {
    pub fn new(levels: Vec<LodLevel<T>>, hysteresis: f32) -> Result<Self, LodError> {
        if levels.is_empty() {
            return Err(LodError::EmptyProfile);
        }
        if !hysteresis.is_finite() || !(0.0..0.5).contains(&hysteresis) {
            return Err(LodError::InvalidHysteresis);
        }
        for (index, level) in levels.iter().enumerate() {
            if !level.max_distance.is_finite()
                || !level.max_distance_squared.is_finite()
                || level.max_distance <= 0.0
            {
                return Err(LodError::InvalidLevelDistance { index });
            }
            if index > 0 && level.max_distance <= levels[index - 1].max_distance {
                return Err(LodError::UnorderedLevelDistances { index });
            }
        }
        Ok(Self { levels, hysteresis })
    }

    pub fn levels(&self) -> &[LodLevel<T>] {
        &self.levels
    }

    pub fn select(
        &self,
        state: &mut LodState,
        distance_squared: f32,
    ) -> Result<Option<LodChoice<'_, T>>, LodError> {
        if !distance_squared.is_finite() || distance_squared < 0.0 {
            return Err(LodError::InvalidDistance);
        }

        let target = self.target_level(distance_squared);
        if !state.initialized {
            state.current = target;
            state.initialized = true;
        } else if let Some(current) = state.current {
            state.current = Some(self.transition(current, distance_squared));
            if state.current == Some(self.levels.len()) {
                state.current = None;
            }
        } else if distance_squared
            < self.levels[self.levels.len() - 1].max_distance_squared
                * (1.0 - self.hysteresis).powi(2)
        {
            state.current = target;
        }

        Ok(state.current.map(|index| LodChoice {
            index,
            level: &self.levels[index],
        }))
    }

    fn target_level(&self, distance_squared: f32) -> Option<usize> {
        self.levels
            .iter()
            .position(|level| distance_squared <= level.max_distance_squared)
    }

    fn transition(&self, mut current: usize, distance_squared: f32) -> usize {
        let far_scale = 1.0 + self.hysteresis;
        while current < self.levels.len()
            && distance_squared > self.levels[current].max_distance_squared * far_scale * far_scale
        {
            current += 1;
        }

        let near_scale = 1.0 - self.hysteresis;
        while current > 0
            && distance_squared
                < self.levels[current - 1].max_distance_squared * near_scale * near_scale
        {
            current -= 1;
        }
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> LodProfile<&'static str> {
        LodProfile::new(
            vec![
                LodLevel::new("lod0", 10.0),
                LodLevel::new("lod1", 30.0),
                LodLevel::new("lod2", 60.0),
            ],
            0.1,
        )
        .unwrap()
    }

    #[test]
    fn selects_per_model_levels_and_culls_past_the_last_range() {
        let profile = profile();
        let mut state = LodState::default();

        assert_eq!(
            profile
                .select(&mut state, 5.0_f32.powi(2))
                .unwrap()
                .unwrap()
                .index,
            0
        );
        assert_eq!(
            profile
                .select(&mut state, 20.0_f32.powi(2))
                .unwrap()
                .unwrap()
                .level
                .asset(),
            &"lod1"
        );
        assert!(profile
            .select(&mut state, 67.0_f32.powi(2))
            .unwrap()
            .is_none());
        assert_eq!(state.current_level(), None);
    }

    #[test]
    fn hysteresis_prevents_lod_flapping_at_boundaries() {
        let profile = profile();
        let mut state = LodState::default();

        profile.select(&mut state, 9.0_f32.powi(2)).unwrap();
        assert_eq!(
            profile
                .select(&mut state, 10.5_f32.powi(2))
                .unwrap()
                .unwrap()
                .index,
            0
        );
        assert_eq!(
            profile
                .select(&mut state, 11.1_f32.powi(2))
                .unwrap()
                .unwrap()
                .index,
            1
        );
        assert_eq!(
            profile
                .select(&mut state, 9.5_f32.powi(2))
                .unwrap()
                .unwrap()
                .index,
            1
        );
        assert_eq!(
            profile
                .select(&mut state, 8.9_f32.powi(2))
                .unwrap()
                .unwrap()
                .index,
            0
        );
    }

    #[test]
    fn rejects_invalid_profiles_and_distances() {
        assert_eq!(
            LodProfile::<()>::new(Vec::new(), 0.1).unwrap_err(),
            LodError::EmptyProfile
        );
        assert_eq!(
            LodProfile::new(vec![LodLevel::new((), 20.0), LodLevel::new((), 10.0)], 0.1)
                .unwrap_err(),
            LodError::UnorderedLevelDistances { index: 1 }
        );

        let profile = profile();
        assert_eq!(
            profile
                .select(&mut LodState::default(), f32::NAN)
                .unwrap_err(),
            LodError::InvalidDistance
        );
    }
}
